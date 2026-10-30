//! recruiter: the desktop app's command line.
//!
//!   recruiter import --email you@example.com [--from mysql://root@127.0.0.1:3306/recruiter_prod]
//!                    [--database path\to\recruiter.sqlite]

use std::path::PathBuf;
use std::process::ExitCode;

use recruiter::{db, import};

/// Herd's MariaDB with the production copy, as set up on Jonas's PC.
const DEFAULT_SOURCE: &str = "mysql://root@127.0.0.1:3306/recruiter_prod";

const USAGE: &str = "usage: recruiter import --email <email> [--from <mysql url>] [--database <file>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("import") => run_import(&args[1..]),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::FAILURE
        }
    }
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
