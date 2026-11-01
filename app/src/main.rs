//! jobfinder: the desktop app's command line.
//!
//!   jobfinder [--port 8765] [--database path\to\jobfinder.sqlite] [--laya path\to\laya]
//!             [--llama path\to\llama] [--browser | --no-browser]
//!             [--tailscale-name pc.tailnet.ts.net]
//!       opens the app's window (desktop/) and serves the app on 127.0.0.1 beside it, for the
//!       Jobnet extension and the pages the window doesn't have yet; with --browser it opens
//!       the web page instead of the window, and with --no-browser neither; with Laya's model
//!       from the laya folder next to the database unless --laya names another; with
//!       the app's own llama.cpp model (llama-server and models\*.gguf) from the llama folder
//!       next to the database unless --llama names another; with
//!       --tailscale-name it also answers `tailscale serve` for that name, so a phone can open it
//!       (docs/phone.md)
//!
//!   jobfinder scan [--database path\to\jobfinder.sqlite]
//!       the job scan, which the setup wizard's Task Scheduler task runs (scan_schedule): every
//!       active job search is searched for new posts, which are matched with the profile
//!       (job_scan::scan_service, the same scan as the dashboard's Scan now)
//!
//!   jobfinder cleanup [--database path\to\jobfinder.sqlite]
//!       removes what the app keeps outside its own file, after saying what it found and asking:
//!       the job scan's schedule, the API keys, and the downloaded and written folders next to the
//!       database; then, asked again, the database itself (cleanup.rs)
//!
//! Nothing is installed: double-click jobfinder.exe (or run the AppImage on Linux) wherever it is
//! kept.
//!
//!   jobfinder --dev ...
//!       (debug builds only) reloads the page in the browser whenever the app restarts, for
//!       `cargo dev` (xtask/), which restarts it under watchexec on every change

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use jobfinder::account::key_vault::KeyVault;
use jobfinder::app_log::{self, Opened};
use jobfinder::context::{Context, DatabaseFile};
use jobfinder::dev_reload::DevReload;
use jobfinder::from_recruiter::{self, Moved};
use jobfinder::instance::{self, Running};
use jobfinder::job_scan::scan_service;
use jobfinder::laya::laya_client::{self, LayaClient};
use jobfinder::llama::{self, hardware::Inventory, llama_server::LlamaServer};
use jobfinder::scan_schedule::{self, ScanSchedule};
use jobfinder::server::{self, AppState};
use jobfinder::taxonomy::Taxonomy;
use jobfinder::{cleanup, db, desktop, extension};

/// How a start shows the app.
#[derive(Debug, Clone, Copy, PartialEq)]
enum FrontEnd {
    /// The desktop window, with the server beside it.
    Window,
    /// The web page, in the default browser (--browser).
    Browser,
    /// Only the server (--no-browser), for `cargo dev` and the phone.
    Neither,
}

const USAGE: &str = "usage: jobfinder [--port <port>] [--database <file>] [--laya <folder>] [--llama <folder>]
                 [--browser | --no-browser] [--tailscale-name <name>] [--dev]
       jobfinder scan [--database <file>]
       jobfinder cleanup [--database <file>]
       jobfinder read-pdf   (a PDF on stdin, its text on stdout; the app runs it)";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("help" | "--help" | "-h") => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some("scan") => run_scan(&args[1..]),
        Some("cleanup") => run_cleanup(&args[1..]),
        Some(jobfinder::pdf_text::child::SUBCOMMAND) => jobfinder::pdf_text::child::run(),
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
            jobfinder::log_line!("{USAGE}");
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

/// Closes the console when this program has it to itself (double-clicked in Explorer), and
/// points stdout and stderr nowhere, so nothing writes to a console that is gone.
#[cfg(windows)]
fn hide_own_console() {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Console::{FreeConsole, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle};
    if !own_console() {
        return;
    }
    // SAFETY: plain calls with no pointers; a null handle is how Windows says "no stdout".
    unsafe {
        let _ = FreeConsole();
        let _ = SetStdHandle(STD_OUTPUT_HANDLE, HANDLE::default());
        let _ = SetStdHandle(STD_ERROR_HANDLE, HANDLE::default());
    }
}

/// Only Windows opens a console for a double-clicked program.
#[cfg(not(windows))]
fn hide_own_console() {}

/// `jobfinder scan`: the job scan (job_scan::scan_service::scan), the same scan as the
/// dashboard's Scan now. It fails (which Task Scheduler shows as the last run result) when it
/// couldn't scan at all; a search or post that failed is said and kept with it.
fn run_scan(args: &[String]) -> ExitCode {
    let Some(database) = database_arg(args) else {
        return ExitCode::FAILURE;
    };
    let moved = from_recruiter::move_data_folder(&database);
    open_log(database.parent().unwrap_or(std::path::Path::new(".")));
    if let Err(why) = said_move(moved) {
        jobfinder::log_line!("{why}");
        return ExitCode::FAILURE;
    }
    // Held while it runs, so `jobfinder cleanup` won't remove the database under it.
    let Ok(_held) = cleanup::hold(&database, cleanup::Hold::Running) else {
        jobfinder::log_line!(
            "JobFinder's cleanup is running on {}, so nothing was scanned.",
            database.display()
        );
        return ExitCode::FAILURE;
    };
    let Some(runtime) = runtime() else {
        return ExitCode::FAILURE;
    };
    let context = match app_context(&runtime, &database, None, None) {
        Ok(context) => context,
        Err(why) => {
            jobfinder::log_line!("{why}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(scan_service::scan(Arc::new(context))) {
        Ok(report) => {
            jobfinder::say!("{}", report.summary());
            ExitCode::SUCCESS
        }
        Err(e) => {
            jobfinder::log_line!("The job scan couldn't run: {}", e.message);
            ExitCode::FAILURE
        }
    }
}

/// `jobfinder cleanup`: says what the app left outside its file, and removes it once the user
/// says yes; the database only after a second yes.
fn run_cleanup(args: &[String]) -> ExitCode {
    let Some(database) = database_arg(args) else {
        return ExitCode::FAILURE;
    };
    if let Err(why) = said_move(from_recruiter::move_data_folder(&database)) {
        jobfinder::log_line!("{why}");
        return ended_by_hand(ExitCode::FAILURE);
    }
    // Held until the database's files are removed, so no JobFinder starts on it meanwhile.
    let Ok(held) = cleanup::hold(&database, cleanup::Hold::Cleanup) else {
        jobfinder::log_line!(
            "JobFinder is running on {}. Close its window, then run the cleanup again.",
            database.display()
        );
        return ended_by_hand(ExitCode::FAILURE);
    };
    let Some(runtime) = runtime() else {
        return ExitCode::FAILURE;
    };
    // The keys' credentials are named by the database's full path, so it is read before the
    // database is gone.
    let vault = KeyVault::credential_manager(db::full_path(&database));
    let user_id = runtime.block_on(user_id_in(&database));
    let schedule = scan_schedule(&database);
    runtime.block_on(take_over_from_recruiter(&database, &vault, user_id, &schedule));
    let found = runtime.block_on(cleanup::find(&database, &schedule, &vault, user_id));

    println!("JobFinder's cleanup, for {}:", database.display());
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
        // Removed while still held, so an app that starts now locks a new file, never one that
        // is gone.
        let _ = std::fs::remove_file(&cleanup::lock_files(&database)[0]);
        drop(held);
    } else {
        println!(
            "Your database, {}, holds your profile, matches and applications, and can't be brought back once deleted.",
            database.display()
        );
        if ask("Delete your database too?") {
            // The locks are held until the database's files are gone, so no app opens them
            // meanwhile, and their files are removed while still held, so an app that starts after
            // locks new files rather than ones that are gone (Windows allows this: std opens files
            // shared for deletion).
            let lock_files = cleanup::lock_files(&database);
            failed.extend(cleanup::remove_database(&found));
            for lock_file in lock_files {
                let _ = std::fs::remove_file(lock_file);
            }
            drop(held);
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
        println!("Done. To remove JobFinder itself, delete its program file.");
        ended_by_hand(ExitCode::SUCCESS)
    } else {
        jobfinder::log_line!("These couldn't be removed:");
        for why in &failed {
            jobfinder::log_line!("- {why}");
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
    let mut front_end = FrontEnd::Window;
    let mut tailscale_name = None;
    let mut dev = false;
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        if flag == "--browser" || flag == "--no-browser" {
            front_end = if flag == "--browser" {
                FrontEnd::Browser
            } else {
                FrontEnd::Neither
            };
            continue;
        }
        if flag == "--dev" {
            if !cfg!(debug_assertions) {
                jobfinder::log_line!(
                    "--dev is for debug builds (cargo dev, or cargo run -- --dev); release builds have the CSS and JS built in."
                );
                return ExitCode::FAILURE;
            }
            dev = true;
            continue;
        }
        let Some(value) = args.next() else {
            jobfinder::log_line!("{flag} needs a value\n{USAGE}");
            return ExitCode::FAILURE;
        };
        match flag.as_str() {
            "--port" => match value.parse() {
                Ok(value) => port = value,
                Err(_) => {
                    jobfinder::log_line!("--port needs a number\n{USAGE}");
                    return ExitCode::FAILURE;
                }
            },
            "--database" => database = PathBuf::from(value),
            "--laya" => laya_folder = Some(PathBuf::from(value)),
            "--llama" => llama_folder = Some(PathBuf::from(value)),
            "--tailscale-name" => tailscale_name = Some(value.trim().to_ascii_lowercase()),
            _ => {
                jobfinder::log_line!("unknown option {flag}\n{USAGE}");
                return ExitCode::FAILURE;
            }
        }
    }
    if front_end == FrontEnd::Window {
        // Double-clicked, the console would stand open behind the window; what the app says
        // goes to the log instead. Started from a terminal, it keeps printing there.
        hide_own_console();
    }
    let moved = from_recruiter::move_data_folder(&database);
    let data_folder = database.parent().unwrap_or(std::path::Path::new(".")).to_path_buf();
    open_log(&data_folder);
    if let Err(why) = said_move(moved) {
        return failed(front_end, why);
    }
    let Some(runtime) = runtime() else {
        return failed(front_end, "The app's runtime couldn't start (see the log).".to_string());
    };
    // The home page opens the setup wizard on a new database, and the match page after that.
    let address = format!("http://127.0.0.1:{port}/");
    match runtime.block_on(instance::running_on(port, &shown_path(&database))) {
        Running::None => {}
        Running::Same => {
            if front_end == FrontEnd::Window && runtime.block_on(instance::bring_to_front(port)) {
                jobfinder::say!("JobFinder is already running; its window is brought to the front.");
                return ExitCode::SUCCESS;
            }
            jobfinder::say!("JobFinder is already running at {address}.");
            if front_end != FrontEnd::Neither && webbrowser::open(&address).is_err() {
                jobfinder::say!("Open {address} in your browser.");
            }
            return ExitCode::SUCCESS;
        }
        Running::Other { database: running } => {
            return failed(
                front_end,
                format!(
                    "Port {port} is taken by a JobFinder on another database ({running}). Close it, or start this one with --port and another number."
                ),
            );
        }
    }
    // Held while the app runs, so `jobfinder cleanup` won't remove the database under it.
    let Ok(_held) = cleanup::hold(&database, cleanup::Hold::Running) else {
        return failed(
            front_end,
            format!(
                "JobFinder's cleanup is running on {}. Let it finish, then start JobFinder again.",
                database.display()
            ),
        );
    };
    let mut context = match app_context(&runtime, &database, laya_folder, llama_folder) {
        Ok(context) => context,
        Err(why) => return failed(front_end, why),
    };
    let folder = data_folder.join(extension::FOLDER);
    context.extension_folder = extension::write(&folder).map_err(|e| {
        jobfinder::log_line!("Couldn't write the Jobnet extension to {}: {e}", folder.display());
        format!("It couldn't be written to {}: {e}", folder.display())
    });
    // Taken before the window opens, so a taken port is said now rather than behind it.
    let listener = match runtime.block_on(server::bind(port)) {
        Ok(listener) => listener,
        Err(e) => {
            return failed(
                front_end,
                format!("Couldn't serve on port {port}: {e}. Is JobFinder already running? Then open {address}."),
            );
        }
    };
    let context = Arc::new(context);
    let mut state = AppState::new(Arc::clone(&context), port);
    let phone_address = tailscale_name.as_ref().map(|name| format!("https://{name}/match"));
    state.tailscale_name = tailscale_name;
    state.dev_reload = dev.then(DevReload::new);
    let (front_sender, front) = tokio::sync::mpsc::unbounded_channel();
    if front_end == FrontEnd::Window {
        state.front = Some(front_sender);
    }
    let state = Arc::new(state);
    runtime.spawn(preload_laya(Arc::clone(&context)));
    let serving = runtime.spawn(server::serve_on(listener, state));
    jobfinder::say!("JobFinder is serving at {address} (data in {}).", database.display());
    if let Some(phone_address) = &phone_address {
        jobfinder::say!("On your phone, with Tailscale on and `tailscale serve` set up: {phone_address}");
    }
    if front_end == FrontEnd::Window {
        return run_window(runtime, context, address, front, serving);
    }
    jobfinder::say!("Close this window to stop it.");
    if front_end == FrontEnd::Browser && webbrowser::open(&address).is_err() {
        jobfinder::say!("Open {address} in your browser.");
    }
    match runtime.block_on(serving) {
        Ok(Ok(())) => ExitCode::SUCCESS,
        Ok(Err(e)) => {
            jobfinder::log_line!("The server stopped: {e}");
            ended_by_hand(ExitCode::FAILURE)
        }
        Err(e) => {
            jobfinder::log_line!("The server stopped: {e}");
            ended_by_hand(ExitCode::FAILURE)
        }
    }
}

/// How long closing the window waits for the app's work to end (a match being written, Laya
/// loading) before the program ends anyway.
const SHUTDOWN_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// Runs the window on this (the main) thread until it is closed, with the server on runtime.
fn run_window(
    runtime: tokio::runtime::Runtime,
    context: Arc<Context>,
    address: String,
    front: tokio::sync::mpsc::UnboundedReceiver<()>,
    serving: tokio::task::JoinHandle<std::io::Result<()>>,
) -> ExitCode {
    // A server that stops while the window is open is logged; the window goes on without it.
    runtime.spawn(async move {
        match serving.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => jobfinder::log_line!("The server stopped: {e}"),
            Err(e) => jobfinder::log_line!("The server stopped: {e}"),
        }
    });
    let launch = desktop::Launch {
        context,
        runtime: runtime.handle().clone(),
        address,
        front,
    };
    let shown = desktop::run(launch);
    runtime.shutdown_timeout(SHUTDOWN_WAIT);
    match shown {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            jobfinder::log_line!(
                "The window couldn't open: {e}. Start JobFinder with --browser to use it in the browser."
            );
            ExitCode::FAILURE
        }
    }
}

/// Says why the app couldn't start (and logs it): in a small window when it was to show its
/// window, else on the console, which waits for Enter when it would close at once.
fn failed(front_end: FrontEnd, why: String) -> ExitCode {
    jobfinder::log_line!("{why}");
    if front_end == FrontEnd::Window {
        if let Err(e) = desktop::show_failure(why) {
            jobfinder::log_line!("The window couldn't open to say so: {e}");
        }
        return ExitCode::FAILURE;
    }
    ended_by_hand(ExitCode::FAILURE)
}

/// What the app's work runs on, for the server and `jobfinder scan`: the database (made when
/// there is none), the local user, the key store, the job scan's schedule, Laya's model and the
/// llama folder (next to the database unless laya_folder or llama_folder names another). Err
/// says why when the database can't be opened.
fn app_context(
    runtime: &tokio::runtime::Runtime,
    database: &std::path::Path,
    laya_folder: Option<PathBuf>,
    llama_folder: Option<PathBuf>,
) -> Result<Context, String> {
    let made_now = !database.exists();
    let db = runtime
        .block_on(db::open(database))
        .map_err(|e| format!("Couldn't open {}: {e}", database.display()))?;
    let user_id = runtime
        .block_on(server::local_user(&db))
        .map_err(|e| format!("Couldn't read the local user: {e}"))?;
    let mut context = Context::new(
        db,
        Arc::new(Taxonomy::load()),
        user_id,
        KeyVault::credential_manager(db::full_path(database)),
    );
    context.database = DatabaseFile {
        path: shown_path(database),
        made_now,
    };
    context.scan_schedule = scan_schedule(database);
    runtime.block_on(take_over_from_recruiter(
        database,
        &context.key_vault,
        user_id,
        &context.scan_schedule,
    ));
    let data_folder = database.parent().unwrap_or(std::path::Path::new("."));
    let laya_folder = laya_folder.unwrap_or_else(|| data_folder.join(laya_client::FOLDER));
    context.laya = LayaClient::from_folder(&laya_folder);
    if context.laya.is_none() {
        jobfinder::say!(
            "No Laya model in {} (no {}), so matches go on without Laya.",
            laya_folder.display(),
            laya_client::ONNX_FILE
        );
    }
    let llama_folder = llama_folder.unwrap_or_else(|| data_folder.join(llama::FOLDER));
    let inventory = Inventory::detect();
    jobfinder::say!(
        "This computer, for the app's own model: {inventory}. Its llama folder is {}.",
        llama_folder.display()
    );
    context.llama = Some(LlamaServer::new(&llama_folder, inventory));
    Ok(context)
}

/// Says (and logs) what the data folder's move from the former name did; Err when it failed.
fn said_move(moved: Result<Moved, String>) -> Result<(), String> {
    match moved? {
        Moved::Nothing => {}
        Moved::Moved { from, to } => jobfinder::say!(
            "Recruiter is now called JobFinder: its data moved from {} to {}.",
            from.display(),
            to.display()
        ),
        Moved::BothThere { former } => jobfinder::say!(
            "Recruiter's former data folder, {}, is left as it is: JobFinder already has its own database. Delete the former folder once you don't need it.",
            former.display()
        ),
    }
    Ok(())
}

/// Moves what the app kept outside the database while it was called Recruiter: the API keys,
/// and on the default database the job scan's schedule. What failed is logged; the app goes on.
async fn take_over_from_recruiter(database: &std::path::Path, vault: &KeyVault, user_id: i64, schedule: &ScanSchedule) {
    match vault.take_over_former_keys(&from_recruiter::former_key_database(database), user_id) {
        Ok(0) => {}
        Ok(moved) => {
            jobfinder::say!("Moved {moved} API key(s) from Recruiter's name in Credential Manager to JobFinder's.")
        }
        Err(why) => jobfinder::log_line!("The API keys kept under Recruiter's name couldn't be moved: {why}"),
    }
    if !from_recruiter::is_default(database) {
        return;
    }
    match schedule.take_over_former().await {
        Ok(None) => {}
        Ok(Some(hours)) => {
            jobfinder::say!("The job scan's schedule (every {hours} hours) moved from Recruiter's name to JobFinder's.")
        }
        Err(why) => jobfinder::log_line!("Recruiter's job scan schedule couldn't be moved to JobFinder: {why}"),
    }
}

/// Loads Laya's model in the background, so the first match needn't wait for it, and says how
/// that went.
async fn preload_laya(context: Arc<Context>) {
    let Some(laya) = &context.laya else {
        return;
    };
    let started = Instant::now();
    match laya.load().await {
        Ok(_) => jobfinder::say!(
            "Laya is loaded from {} ({:.1} s).",
            laya.folder().display(),
            started.elapsed().as_secs_f64()
        ),
        Err(e) => jobfinder::log_line!("{e}"),
    }
}

/// The job scan's Task Scheduler task, running this jobfinder.exe on database.
fn scan_schedule(database: &std::path::Path) -> ScanSchedule {
    // The AppImage on Linux: the program's own path is inside it, and gone once it ends.
    let exe = scan_schedule::running_program().unwrap_or_else(|e| {
        jobfinder::log_line!(
            "This program's own path couldn't be read ({e}), so the job scan's task runs jobfinder.exe from PATH."
        );
        PathBuf::from("jobfinder.exe")
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
    let full = db::full_path(database);
    match full.strip_prefix(VERBATIM_PREFIX) {
        Some(plain) => plain.to_string(),
        None => full,
    }
}

/// What std::fs::canonicalize puts before a path on Windows.
const VERBATIM_PREFIX: &str = r"\\?\";

/// Opens the log (app_log) and says where it goes, or why it couldn't be opened.
fn open_log(folder: &std::path::Path) {
    match app_log::open(folder) {
        Ok(Opened::File(path)) => jobfinder::say!("Logging to {}.", path.display()),
        Ok(Opened::Journal) => jobfinder::say!("Logging to the systemd journal: journalctl --user -t jobfinder"),
        Ok(Opened::Console) => jobfinder::say!("Logging to the systemd journal, as this program's output."),
        Err(e) => jobfinder::log_line!(
            "The log file couldn't be opened in {}: {e}. What the app says goes only to this window.",
            folder.display()
        ),
    }
}

/// The async runtime the database and the server run on, or None (said why) when it can't start.
fn runtime() -> Option<tokio::runtime::Runtime> {
    tokio::runtime::Runtime::new()
        .map_err(|e| jobfinder::log_line!("Couldn't start: {e}"))
        .ok()
}
