//! recruiter: the desktop app's command line.
//!
//!   recruiter [--port 8765] [--database path\to\recruiter.sqlite] [--no-browser]
//!       serves the app on 127.0.0.1 and opens it in the default browser
//!   recruiter import --email you@example.com [--from mysql://root@127.0.0.1:3306/recruiter_prod]
//!                    [--database path\to\recruiter.sqlite]

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use recruiter::server::{self, AppState};
use recruiter::taxonomy::Taxonomy;
use recruiter::{db, extension, import};

/// Herd's MariaDB with the production copy, as set up on Jonas's PC.
const DEFAULT_SOURCE: &str = "mysql://root@127.0.0.1:3306/recruiter_prod";

const USAGE: &str = "usage: recruiter [--port <port>] [--database <file>] [--no-browser]
       recruiter import --email <email> [--from <mysql url>] [--database <file>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("import") => run_import(&args[1..]),
        Some("help" | "--help" | "-h") => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        _ => run_server(&args),
    }
}

fn run_server(args: &[String]) -> ExitCode {
    let mut port = server::DEFAULT_PORT;
    let mut database = db::default_path();
    let mut browser = true;
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        if flag == "--no-browser" {
            browser = false;
            continue;
        }
        let Some(value) = args.next() else {
            eprintln!("{flag} needs a value\n{USAGE}");
            return ExitCode::FAILURE;
        };
        match flag.as_str() {
            "--port" => match value.parse() {
                Ok(value) => port = value,
                Err(_) => {
                    eprintln!("--port needs a number\n{USAGE}");
                    return ExitCode::FAILURE;
                }
            },
            "--database" => database = PathBuf::from(value),
            _ => {
                eprintln!("unknown option {flag}\n{USAGE}");
                return ExitCode::FAILURE;
            }
        }
    }
    let db = match db::open(&database) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("Couldn't open {}: {e}", database.display());
            return ExitCode::FAILURE;
        }
    };
    let user_id = match server::local_user(&db) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("Couldn't read the local user: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut state = AppState::new(db, Arc::new(Taxonomy::load()), user_id, port);
    let folder = database
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join(extension::FOLDER);
    state.extension_folder = extension::write(&folder).map_err(|e| {
        eprintln!("Couldn't write the Jobnet extension to {}: {e}", folder.display());
        format!("It couldn't be written to {}: {e}", folder.display())
    });
    let state = Arc::new(state);
    let address = format!("http://127.0.0.1:{port}/cv_match");
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("Couldn't start: {e}");
            return ExitCode::FAILURE;
        }
    };
    runtime.block_on(async {
        let serving = tokio::spawn(server::serve(state));
        println!(
            "Recruiter is running at {address} (data in {}). Close this window to stop it.",
            database.display()
        );
        if browser && webbrowser::open(&address).is_err() {
            println!("Open {address} in your browser.");
        }
        match serving.await {
            Ok(Ok(())) => ExitCode::SUCCESS,
            Ok(Err(e)) => {
                eprintln!("Couldn't serve on port {port}: {e}. Is Recruiter already running? Then open {address}.");
                ExitCode::FAILURE
            }
            Err(e) => {
                eprintln!("The server stopped: {e}");
                ExitCode::FAILURE
            }
        }
    })
}

fn run_import(args: &[String]) -> ExitCode {
    let mut email = None;
    let mut source = DEFAULT_SOURCE.to_string();
    let mut database = db::default_path();
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        let Some(value) = args.next() else {
            eprintln!("{flag} needs a value\n{USAGE}");
            return ExitCode::FAILURE;
        };
        match flag.as_str() {
            "--email" => email = Some(value.clone()),
            "--from" => source = value.clone(),
            "--database" => database = PathBuf::from(value),
            _ => {
                eprintln!("unknown option {flag}\n{USAGE}");
                return ExitCode::FAILURE;
            }
        }
    }
    let Some(email) = email else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };

    let result = db::open(&database)
        .map_err(import::Error::Sqlite)
        .and_then(|mut target| import::import(&source, &email, &mut target));
    match result {
        Ok(report) => {
            println!("Imported into {}", database.display());
            for (table, rows) in &report.rows {
                println!("  {table}: {rows}");
            }
            if report.skipped_llm_key {
                println!(
                    "Your LLM key isn't imported (it is encrypted with the server's secret); add it again in the app."
                );
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Import failed, nothing was written: {e}");
            ExitCode::FAILURE
        }
    }
}
