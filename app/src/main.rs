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
//!   recruiter install [--database path\to\recruiter.sqlite]
//!       puts the program in %LOCALAPPDATA%\Programs\Recruiter with a Start menu shortcut and an
//!       entry in Windows' installed apps, then starts it there (install); double-clicking the
//!       release's Recruiter-setup.exe, which is this program, does the same
//!
//!   recruiter uninstall [--database path\to\recruiter.sqlite]
//!       undoes that, and keeps the data
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
use recruiter::install;
use recruiter::laya::laya_client::{self, LayaClient};
use recruiter::llama::{self, hardware::Inventory, llama_server::LlamaServer};
use recruiter::scan_schedule::ScanSchedule;
use recruiter::server::{self, AppState, DatabaseFile};
use recruiter::taxonomy::Taxonomy;
use recruiter::{db, extension};

const USAGE: &str = "usage: recruiter [--port <port>] [--database <file>] [--laya <folder>] [--llama <folder>]
                 [--no-browser] [--tailscale-name <name>] [--dev]
       recruiter scan [--database <file>]
       recruiter install [--database <file>]
       recruiter uninstall [--database <file>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("help" | "--help" | "-h") => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some("scan") => run_scan(&args[1..]),
        Some("install") => run_install(&args[1..]),
        Some("uninstall") => run_uninstall(&args[1..]),
        None if started_as_setup() => run_install(&[]),
        _ => run_server(&args),
    }
}

/// Whether this program was started as Recruiter-setup.exe, the release's installer.
fn started_as_setup() -> bool {
    std::env::current_exe().is_ok_and(|exe| install::is_setup_name(&exe))
}

/// `recruiter install`: installs this program, then starts the installed copy in a window of
/// its own, which opens the dashboard.
fn run_install(args: &[String]) -> ExitCode {
    let Some(database) = database_arg(args) else {
        return ExitCode::FAILURE;
    };
    let Some(runtime) = runtime() else {
        return ended_by_hand(ExitCode::FAILURE);
    };
    // Whole, even before the file exists: the shortcut starts in the install folder, so a
    // relative path would name another file there.
    let database = std::path::absolute(&database).unwrap_or(database);
    let database = PathBuf::from(shown_path(&database));
    let named = (!args.is_empty()).then_some(database.as_path());
    let installed = match runtime.block_on(install::install(&database, named)) {
        Ok(installed) => installed,
        Err(why) => {
            eprintln!("Recruiter couldn't be installed: {why}");
            return ended_by_hand(ExitCode::FAILURE);
        }
    };
    for line in &installed.done {
        println!("{line}");
    }
    match start_in_own_window(&installed.exe, &installed.arguments) {
        Ok(()) => {
            println!("Recruiter is starting from the Start menu's copy, and opens its dashboard in your browser.");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Recruiter is installed, but it couldn't be started ({e}). Start it from the Start menu.");
            ended_by_hand(ExitCode::FAILURE)
        }
    }
}

/// `recruiter uninstall`: removes what install added, and keeps the data.
fn run_uninstall(args: &[String]) -> ExitCode {
    let Some(database) = database_arg(args) else {
        return ExitCode::FAILURE;
    };
    let Some(runtime) = runtime() else {
        return ended_by_hand(ExitCode::FAILURE);
    };
    let uninstalled = match runtime.block_on(install::uninstall(&database)) {
        Ok(uninstalled) => uninstalled,
        Err(why) => {
            eprintln!("Recruiter couldn't be uninstalled: {why}");
            return ended_by_hand(ExitCode::FAILURE);
        }
    };
    for line in &uninstalled.done {
        println!("{line}");
    }
    let code = ended_by_hand(ExitCode::SUCCESS);
    // Last, after the wait for Enter: the clean-up's delay counts from here.
    if let Some(folder) = &uninstalled.remove_after_exit
        && let Err(why) = install::remove_after_exit(folder)
    {
        eprintln!("{why}");
        return ExitCode::FAILURE;
    }
    code
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

/// Starts exe with arguments in a console window of its own, so it keeps running (and says
/// "close this window to stop it") after this one has ended.
fn start_in_own_window(exe: &std::path::Path, arguments: &[String]) -> std::io::Result<()> {
    let mut command = std::process::Command::new(exe);
    command.args(arguments);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        /// A process with a console window of its own (CREATE_NEW_CONSOLE).
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
        command.creation_flags(CREATE_NEW_CONSOLE);
    }
    command.spawn().map(|_| ())
}

/// code, after waiting for Enter when this program has its console to itself: started from
/// Explorer or the installed apps list, whose window would otherwise close before what it said
/// could be read.
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
    let made_now = !database.exists();
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

/// The job scan's Task Scheduler task, running this recruiter.exe on database.
fn scan_schedule(database: &std::path::Path) -> ScanSchedule {
    let exe = std::env::current_exe().unwrap_or_else(|e| {
        eprintln!(
            "This program's own path couldn't be read ({e}), so the job scan's task runs recruiter.exe from PATH."
        );
        PathBuf::from("recruiter.exe")
    });
    ScanSchedule::task_scheduler(&exe, std::path::Path::new(&shown_path(database)))
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
