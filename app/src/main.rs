//! recruiter: the desktop app's command line.
//!
//!   recruiter [--port 8765] [--database path\to\recruiter.sqlite] [--laya path\to\laya]
//!             [--llama path\to\llama] [--no-browser] [--tailscale-name pc.tailnet.ts.net]
//!       serves the app on 127.0.0.1 and opens it in the default browser, with Laya's model
//!       from the laya folder next to the database unless --laya names another; with
//!       the app's own llama.cpp model (llama-server and models\*.gguf) from the llama folder
//!       next to the database unless --llama names another; with
//!       --tailscale-name it also answers `tailscale serve` for that name, so a phone can open it
//!       (docs/phone.md)

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use recruiter::account::key_vault::KeyVault;
use recruiter::laya::laya_client::{self, LayaClient};
use recruiter::llama::{self, hardware::Inventory, llama_server::LlamaServer};
use recruiter::server::{self, AppState};
use recruiter::taxonomy::Taxonomy;
use recruiter::{db, extension};

const USAGE: &str = "usage: recruiter [--port <port>] [--database <file>] [--laya <folder>] [--llama <folder>]
                 [--no-browser] [--tailscale-name <name>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
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
    let mut laya_folder = None;
    let mut llama_folder = None;
    let mut browser = true;
    let mut tailscale_name = None;
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
            "--laya" => laya_folder = Some(PathBuf::from(value)),
            "--llama" => llama_folder = Some(PathBuf::from(value)),
            "--tailscale-name" => tailscale_name = Some(value.trim().to_ascii_lowercase()),
            _ => {
                eprintln!("unknown option {flag}\n{USAGE}");
                return ExitCode::FAILURE;
            }
        }
    }
    let Some(runtime) = runtime() else {
        return ExitCode::FAILURE;
    };
    let db = match runtime.block_on(db::open(&database)) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("Couldn't open {}: {e}", database.display());
            return ExitCode::FAILURE;
        }
    };
    let user_id = match runtime.block_on(server::local_user(&db)) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("Couldn't read the local user: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut state = AppState::new(
        db,
        Arc::new(Taxonomy::load()),
        user_id,
        port,
        KeyVault::credential_manager(full_path(&database)),
    );
    let data_folder = database.parent().unwrap_or(std::path::Path::new("."));
    let laya_folder = laya_folder.unwrap_or_else(|| data_folder.join(laya_client::FOLDER));
    state.laya = LayaClient::from_folder(&laya_folder);
    if state.laya.is_none() {
        println!(
            "No Laya model in {} (no {}), so matches go on without Laya.",
            laya_folder.display(),
            laya_client::ONNX_FILE
        );
    }
    let llama_folder = llama_folder.unwrap_or_else(|| data_folder.join(llama::FOLDER));
    let inventory = Inventory::detect();
    println!(
        "This computer, for the app's own model: {inventory}. Its llama folder is {}.",
        llama_folder.display()
    );
    state.llama = Some(LlamaServer::new(&llama_folder, inventory));
    let folder = data_folder.join(extension::FOLDER);
    state.extension_folder = extension::write(&folder).map_err(|e| {
        eprintln!("Couldn't write the Jobnet extension to {}: {e}", folder.display());
        format!("It couldn't be written to {}: {e}", folder.display())
    });
    let phone_address = tailscale_name.as_ref().map(|name| format!("https://{name}/match"));
    state.tailscale_name = tailscale_name;
    let state = Arc::new(state);
    let address = format!("http://127.0.0.1:{port}/match");
    runtime.block_on(async {
        tokio::spawn(preload_laya(Arc::clone(&state)));
        let serving = tokio::spawn(server::serve(state));
        println!(
            "Recruiter is running at {address} (data in {}). Close this window to stop it.",
            database.display()
        );
        if let Some(phone_address) = &phone_address {
            println!("On your phone, with Tailscale on and `tailscale serve` set up: {phone_address}");
        }
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

/// Loads Laya's model in the background, so the first match needn't wait for it, and says how
/// that went.
async fn preload_laya(state: Arc<AppState>) {
    let Some(laya) = &state.laya else {
        return;
    };
    let started = Instant::now();
    match laya.load().await {
        Ok(_) => println!(
            "Laya is loaded from {} ({:.1} s).",
            laya.folder().display(),
            started.elapsed().as_secs_f64()
        ),
        Err(e) => eprintln!("{e}"),
    }
}

/// The database file's full path, which names its key's credential; as given when it can't be
/// resolved.
fn full_path(database: &std::path::Path) -> String {
    std::fs::canonicalize(database)
        .unwrap_or_else(|_| database.to_path_buf())
        .display()
        .to_string()
}

/// The async runtime the database and the server run on, or None (said why) when it can't start.
fn runtime() -> Option<tokio::runtime::Runtime> {
    tokio::runtime::Runtime::new()
        .map_err(|e| eprintln!("Couldn't start: {e}"))
        .ok()
}
