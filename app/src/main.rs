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
//!
//!   recruiter scan [--database path\to\recruiter.sqlite]
//!       the job scan, which the setup wizard's Task Scheduler task runs (scan_schedule); it
//!       isn't in the desktop app yet, so for now it only says so
//!
//!   recruiter cleanup [--database path\to\recruiter.sqlite]
//!       removes what the app keeps outside its own file, after saying what it found and asking:
//!       the job scan's schedule, the API keys, and the downloaded and written folders next to the
//!       database; then, asked again, the database itself (cleanup.rs)
//!
//! Nothing is installed: double-click recruiter.exe (or run the AppImage on Linux) wherever it is
//! kept.
//!
//!   recruiter --dev ...
//!       (debug builds only) reloads the page in the browser whenever the app restarts, for
//!       `cargo dev` (xtask/), which restarts it under watchexec on every change

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use recruiter::account::key_vault::KeyVault;
use recruiter::dev_reload::DevReload;
use recruiter::laya::laya_client::{self, LayaClient};
use recruiter::llama::{self, hardware::Inventory, llama_server::LlamaServer};
use recruiter::scan_schedule::{self, ScanSchedule};
use recruiter::server::{self, AppState, DatabaseFile};
use recruiter::taxonomy::Taxonomy;
use recruiter::{cleanup, db, extension};

const USAGE: &str = "usage: recruiter [--port <port>] [--database <file>] [--laya <folder>] [--llama <folder>]
                 [--no-browser] [--tailscale-name <name>] [--dev]
       recruiter scan [--database <file>]
       recruiter cleanup [--database <file>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("help" | "--help" | "-h") => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some("scan") => run_scan(&args[1..]),
        Some("cleanup") => run_cleanup(&args[1..]),
        _ => run_server(&args),
    }
}

/// The database a subcommand's `[--database <file>]` names, or the default one; None (with the
/// usage said) for anything else.
fn database_arg(args: &[String]) -> Option<PathBuf> {
    match args {
        [] => Some(db::default_path()),
        [flag, value] if flag == "--database" => Some(PathBuf::from(value)),
        _ => {
            eprintln!("{USAGE}");
            None
        }
    }
}

/// code, after waiting for Enter when this program has its console to itself: double-clicked in
/// Explorer, whose window would otherwise close before what it said could be read.
fn ended_by_hand(code: ExitCode) -> ExitCode {
    if own_console() {
        println!("Press Enter to close this window.");
        let _ = std::io::stdin().read_line(&mut String::new());
    }
    code
}

/// Whether this program is the only one on its console, so the console closes when it ends.
#[cfg(windows)]
fn own_console() -> bool {
    let mut processes = [0_u32; 2];
    // SAFETY: the list is a valid buffer of the length passed with it.
    let count = unsafe { windows::Win32::System::Console::GetConsoleProcessList(&mut processes) };
    count == 1
}

#[cfg(not(windows))]
fn own_console() -> bool {
    false
}

/// `recruiter scan`: the job scan. Until it is ported it opens the database, so a wrong
/// --database shows in Task Scheduler's last run result, and says it isn't here yet.
fn run_scan(args: &[String]) -> ExitCode {
    let Some(database) = database_arg(args) else {
        return ExitCode::FAILURE;
    };
    let Some(runtime) = runtime() else {
        return ExitCode::FAILURE;
    };
    if let Err(e) = runtime.block_on(db::open(&database)) {
        eprintln!("Couldn't open {}: {e}", database.display());
        return ExitCode::FAILURE;
    }
    eprintln!("The job scan isn't in the desktop app yet, so nothing was scanned.");
    ExitCode::FAILURE
}

/// `recruiter cleanup`: says what the app left outside its file, and removes it once the user
/// says yes; the database only after a second yes.
fn run_cleanup(args: &[String]) -> ExitCode {
    let Some(database) = database_arg(args) else {
        return ExitCode::FAILURE;
    };
    // Held until the database's files are removed, so no Recruiter starts on it meanwhile.
    let Ok(held) = cleanup::hold(&database, cleanup::Hold::Cleanup) else {
        eprintln!(
            "Recruiter is running on {}. Close its window, then run the cleanup again.",
            database.display()
        );
        return ended_by_hand(ExitCode::FAILURE);
    };
    let Some(runtime) = runtime() else {
        return ExitCode::FAILURE;
    };
    // The keys' credentials are named by the database's full path, so it is read before the
    // database is gone.
    let vault = KeyVault::credential_manager(full_path(&database));
    let user_id = runtime.block_on(user_id_in(&database));
    let schedule = scan_schedule(&database);
    let found = runtime.block_on(cleanup::find(&database, &schedule, &vault, user_id));

    println!("Recruiter's cleanup, for {}:", database.display());
    match (found.schedule, &found.schedule_error) {
        (Some(hours), _) => println!("- the job scan's schedule (every {hours} hours)"),
        (None, Some(why)) => println!("  (the job scan's schedule couldn't be read: {why})"),
        (None, None) => {}
    }
    for provider in &found.keys {
        println!("- the {} key", cleanup::key_name(provider));
    }
    if let Some(why) = &found.keys_error {
        println!("  (the API keys couldn't be read: {why})");
    }
    for folder in &found.folders {
        println!("- {}", folder.display());
    }
    let mut failed = Vec::new();
    if !found.anything_besides_the_database() {
        println!("No schedule, keys or downloads to remove.");
    } else if ask("Remove these?") {
        failed.extend(runtime.block_on(cleanup::remove_besides_the_database(&found, &schedule, &vault, user_id)));
    } else {
        println!("Nothing was removed.");
        return ended_by_hand(ExitCode::SUCCESS);
    }
    if !database.exists() {
        // Only the lock this cleanup took is left.
        drop(held);
        let _ = std::fs::remove_file(cleanup::lock_file(&database));
    } else {
        println!(
            "Your database, {}, holds your profile, matches and applications, and can't be brought back once deleted.",
            database.display()
        );
        if ask("Delete your database too?") {
            // The lock is held until the database's files are gone, so no app opens them meanwhile;
            // its own file goes last, once let go (Windows won't delete an open file).
            let lock_file = cleanup::lock_file(&database);
            failed.extend(cleanup::remove_database(&found));
            drop(held);
            let _ = std::fs::remove_file(lock_file);
            let default_path = db::default_path();
            let default_folder = default_path.parent().unwrap_or(std::path::Path::new("."));
            if failed.is_empty() && cleanup::remove_default_folder(&database, default_folder) {
                println!("Removed the empty data folder too.");
            }
        } else {
            println!("Your database stays.");
        }
    }
    println!("The Jobnet extension, if you loaded it, is removed in Edge's extensions page (edge://extensions).");
    if failed.is_empty() {
        println!("Done. To remove Recruiter itself, delete its program file.");
        ended_by_hand(ExitCode::SUCCESS)
    } else {
        eprintln!("These couldn't be removed:");
        for why in &failed {
            eprintln!("- {why}");
        }
        ended_by_hand(ExitCode::FAILURE)
    }
}

/// Whether the user answers yes to question; no unless they type y or yes.
fn ask(question: &str) -> bool {
    use std::io::Write;
    print!("{question} [y/N] ");
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() {
        return false;
    }
    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

/// The local user's id in database, read without changing the file; 1 (the first user) when it
/// can't be read.
async fn user_id_in(database: &std::path::Path) -> i64 {
    if !database.exists() {
        return 1;
    }
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(database)
        .read_only(true);
    let Ok(pool) = sqlx::SqlitePool::connect_with(options).await else {
        return 1;
    };
    let id: Option<i64> = sqlx::query_scalar("SELECT MIN(id) FROM trongate_users")
        .fetch_one(&pool)
        .await
        .ok()
        .flatten();
    pool.close().await;
    id.unwrap_or(1)
}

fn run_server(args: &[String]) -> ExitCode {
    let mut port = server::DEFAULT_PORT;
    let mut database = db::default_path();
    let mut laya_folder = None;
    let mut llama_folder = None;
    let mut browser = true;
    let mut tailscale_name = None;
    let mut dev = false;
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        if flag == "--no-browser" {
            browser = false;
            continue;
        }
        if flag == "--dev" {
            if !cfg!(debug_assertions) {
                eprintln!(
                    "--dev is for debug builds (cargo dev, or cargo run -- --dev); release builds have the CSS and JS built in."
                );
                return ExitCode::FAILURE;
            }
            dev = true;
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
    // Held while the app runs, so `recruiter cleanup` won't remove the database under it.
    let Ok(_held) = cleanup::hold(&database, cleanup::Hold::Running) else {
        eprintln!(
            "Recruiter's cleanup is running on {}. Let it finish, then start Recruiter again.",
            database.display()
        );
        return ended_by_hand(ExitCode::FAILURE);
    };
    let made_now = !database.exists();
    let db = match runtime.block_on(db::open(&database)) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("Couldn't open {}: {e}", database.display());
            return ended_by_hand(ExitCode::FAILURE);
        }
    };
    let user_id = match runtime.block_on(server::local_user(&db)) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("Couldn't read the local user: {e}");
            return ended_by_hand(ExitCode::FAILURE);
        }
    };
    let mut state = AppState::new(
        db,
        Arc::new(Taxonomy::load()),
        user_id,
        port,
        KeyVault::credential_manager(full_path(&database)),
    );
    state.database = DatabaseFile {
        path: shown_path(&database),
        made_now,
    };
    state.scan_schedule = scan_schedule(&database);
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
    state.dev_reload = dev.then(DevReload::new);
    let state = Arc::new(state);
    // The home page opens the setup wizard on a new database, and the match page after that.
    let address = format!("http://127.0.0.1:{port}/");
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
                ended_by_hand(ExitCode::FAILURE)
            }
            Err(e) => {
                eprintln!("The server stopped: {e}");
                ended_by_hand(ExitCode::FAILURE)
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

/// The job scan's Task Scheduler task, running this recruiter.exe on database.
fn scan_schedule(database: &std::path::Path) -> ScanSchedule {
    // The AppImage on Linux: the program's own path is inside it, and gone once it ends.
    let exe = scan_schedule::running_program().unwrap_or_else(|e| {
        eprintln!(
            "This program's own path couldn't be read ({e}), so the job scan's task runs recruiter.exe from PATH."
        );
        PathBuf::from("recruiter.exe")
    });
    ScanSchedule::system(
        &exe,
        std::path::Path::new(&shown_path(database)),
        scan_schedule::extracted_appimage(),
    )
}

/// The database file's full path as people and other programs read it: without the \\?\ that
/// Windows' canonical paths start with.
fn shown_path(database: &std::path::Path) -> String {
    let full = full_path(database);
    match full.strip_prefix(VERBATIM_PREFIX) {
        Some(plain) => plain.to_string(),
        None => full,
    }
}

/// What std::fs::canonicalize puts before a path on Windows.
const VERBATIM_PREFIX: &str = r"\\?\";

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
