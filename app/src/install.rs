//! install: puts Recruiter on this computer as a program. `recruiter install` (or double-clicking
//! Recruiter-setup.exe, the same program under the release's other name) does four things:
//!
//! 1. copies the program to %LOCALAPPDATA%\Programs\Recruiter\recruiter.exe, a folder that needs
//!    no administrator and doesn't move when a build folder is cleaned;
//! 2. adds Recruiter to the Start menu;
//! 3. adds it to Windows' installed apps, whose Uninstall runs `recruiter.exe uninstall`;
//! 4. points the job scan's Task Scheduler task at the copy, when there is a task already;
//!
//! then starts the copy, which opens the setup wizard on a new database. The rest of the setup
//! (the database, the AI model, the CV, the schedule) is the wizard's, so installing again is how
//! an update is put in place: it replaces the program and keeps everything else.
//!
//! `recruiter uninstall` undoes the four and leaves the data: the database, the models and the
//! API keys stay, and it says where.
//!
//! Only Windows for now. The plain parts (paths, the registry values, the shortcut's script) are
//! functions of their own, so the tests check them on any computer.

use std::path::{Path, PathBuf};

use crate::scan_schedule::ScanSchedule;

/// The program's file name in the install folder.
pub const EXE_NAME: &str = "recruiter.exe";

/// What a file name ends with (before .exe) when the program was started as the installer, as
/// the release's Recruiter-setup.exe.
pub const SETUP_SUFFIX: &str = "-setup";

/// The name in the Start menu and the installed apps list.
const DISPLAY_NAME: &str = "Recruiter";

/// Who the installed apps list says made it.
const PUBLISHER: &str = "sasin91";

/// The installed apps list's key for Recruiter, under HKEY_CURRENT_USER: per user, so it needs
/// no administrator, like the install folder.
#[cfg_attr(not(windows), allow(dead_code))]
const UNINSTALL_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Recruiter";

/// Bytes in a KB, as the installed apps list takes the program's size.
const BYTES_PER_KB: u64 = 1024;

/// How many pings (a second apart, the first at once) the uninstall's clean-up waits before it
/// deletes the install folder: long enough for `recruiter.exe uninstall`, which can't delete its
/// own file while it runs, to have ended.
const CLEAN_UP_WAIT_PINGS: u32 = 3;

/// Where Recruiter is installed, and the files it adds beside it.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// %LOCALAPPDATA%\Programs\Recruiter
    pub folder: PathBuf,
    /// The Start menu's shortcut: %APPDATA%\Microsoft\Windows\Start Menu\Programs\Recruiter.lnk
    pub shortcut: PathBuf,
}

impl Layout {
    /// The layout for this user: under local_data (%LOCALAPPDATA%) and roaming_data (%APPDATA%).
    pub fn under(local_data: &Path, roaming_data: &Path) -> Self {
        Layout {
            folder: local_data.join("Programs").join(DISPLAY_NAME),
            shortcut: roaming_data
                .join(r"Microsoft\Windows\Start Menu\Programs")
                .join(format!("{DISPLAY_NAME}.lnk")),
        }
    }

    /// This user's layout, or None when Windows doesn't say where their data folders are.
    pub fn for_this_user() -> Option<Self> {
        Some(Layout::under(&dirs::data_local_dir()?, &dirs::data_dir()?))
    }

    /// The installed program.
    pub fn exe(&self) -> PathBuf {
        self.folder.join(EXE_NAME)
    }
}

/// Whether the program at exe was started as the installer: Recruiter-setup.exe.
pub fn is_setup_name(exe: &Path) -> bool {
    exe.file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| stem.to_ascii_lowercase().ends_with(SETUP_SUFFIX))
}

/// A value of the installed apps list's key, as `reg add` takes it.
#[derive(Debug, Clone, PartialEq)]
pub struct RegValue {
    pub name: &'static str,
    /// REG_SZ or REG_DWORD.
    pub kind: &'static str,
    pub data: String,
}

/// The installed apps list's values for Recruiter at exe, whose file is size bytes.
pub fn uninstall_values(layout: &Layout, size: u64) -> Vec<RegValue> {
    let exe = layout.exe().display().to_string();
    let text = |name, data: String| RegValue {
        name,
        kind: "REG_SZ",
        data,
    };
    let number = |name, data: u64| RegValue {
        name,
        kind: "REG_DWORD",
        data: data.to_string(),
    };
    vec![
        text("DisplayName", DISPLAY_NAME.to_string()),
        text("DisplayVersion", env!("CARGO_PKG_VERSION").to_string()),
        text("Publisher", PUBLISHER.to_string()),
        text("InstallLocation", layout.folder.display().to_string()),
        text("DisplayIcon", exe.clone()),
        text("UninstallString", format!(r#""{exe}" uninstall"#)),
        number("NoModify", 1),
        number("NoRepair", 1),
        number("EstimatedSize", size.div_ceil(BYTES_PER_KB)),
    ]
}

/// The installed program's arguments for database: none for the default one, else
/// `--database <file>`, so the Start menu's copy opens the database the scan's task scans.
pub fn arguments_for(database: Option<&Path>) -> Vec<String> {
    match database {
        Some(database) => vec!["--database".to_string(), database.display().to_string()],
        None => Vec::new(),
    }
}

/// The PowerShell that makes the Start menu's shortcut to the installed program, started with
/// arguments.
pub fn shortcut_script(layout: &Layout, arguments: &[String]) -> String {
    let arguments: Vec<String> = arguments
        .iter()
        .map(|argument| {
            if argument.contains(' ') {
                format!(r#""{argument}""#)
            } else {
                argument.clone()
            }
        })
        .collect();
    format!(
        "$shortcut = (New-Object -ComObject WScript.Shell).CreateShortcut({}); \
         $shortcut.TargetPath = {}; $shortcut.Arguments = {}; $shortcut.WorkingDirectory = {}; \
         $shortcut.Description = {}; $shortcut.Save()",
        powershell_string(&layout.shortcut.display().to_string()),
        powershell_string(&layout.exe().display().to_string()),
        powershell_string(&arguments.join(" ")),
        powershell_string(&layout.folder.display().to_string()),
        powershell_string(DISPLAY_NAME),
    )
}

/// text as a PowerShell string that is taken as it is: in single quotes, its own doubled.
fn powershell_string(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// The cmd.exe line that deletes the install folder once the uninstall has ended.
pub fn clean_up_line(folder: &Path) -> String {
    format!(
        r#"/c ping -n {CLEAN_UP_WAIT_PINGS} 127.0.0.1 > nul & rmdir /s /q "{}""#,
        folder.display()
    )
}

/// What installing did, for the installer to say.
#[derive(Debug, Default)]
pub struct Installed {
    pub exe: PathBuf,
    /// What the installed program is started with (arguments_for).
    pub arguments: Vec<String>,
    /// Lines of what was done, in order.
    pub done: Vec<String>,
}

/// Installs the program that is running, as the module says, with the job scan's task (when
/// there is one) running the copy on database. named_database is the database when it isn't the
/// default one, which the shortcut then opens too. Starting the copy is the caller's.
pub async fn install(database: &Path, named_database: Option<&Path>) -> Result<Installed, String> {
    let layout = Layout::for_this_user()
        .ok_or("Windows didn't say where this user's AppData folders are, so there's nowhere to install to.")?;
    let running = std::env::current_exe().map_err(|e| format!("This program's own path couldn't be read: {e}"))?;
    let mut installed = Installed {
        exe: layout.exe(),
        arguments: arguments_for(named_database),
        done: Vec::new(),
    };
    if same_file(&running, &layout.exe()) {
        installed
            .done
            .push(format!("Recruiter is already in {}.", layout.folder.display()));
    } else {
        copy_program(&running, &layout).await?;
        installed
            .done
            .push(format!("Copied Recruiter to {}.", layout.folder.display()));
    }
    system::make_shortcut(&layout, &installed.arguments).await?;
    installed.done.push("Added Recruiter to the Start menu.".to_string());
    let size = std::fs::metadata(layout.exe()).map(|meta| meta.len()).unwrap_or(0);
    system::write_uninstall_entry(&uninstall_values(&layout, size)).await?;
    installed
        .done
        .push("Added Recruiter to Windows' installed apps, where it can be uninstalled.".to_string());
    let schedule = ScanSchedule::task_scheduler(&layout.exe(), database);
    match schedule.interval_hours().await {
        Ok(Some(hours)) => {
            schedule
                .register(hours)
                .await
                .map_err(|why| format!("The job scan's task couldn't be moved to the installed copy: {why}"))?;
            installed
                .done
                .push("The job scan's task now runs the installed copy.".to_string());
        }
        // No task yet: the wizard's last step makes it, from the installed copy.
        Ok(None) => {}
        Err(why) => eprintln!("install: the job scan's task couldn't be read, so it was left as it is: {why}"),
    }
    Ok(installed)
}

/// What uninstalling did, for the uninstaller to say.
#[derive(Debug, Default)]
pub struct Uninstalled {
    /// Lines of what was done, in order.
    pub done: Vec<String>,
    /// The install folder, when this program runs from it: Windows won't delete a running
    /// program's file, so the caller deletes it with remove_after_exit as the very last thing.
    pub remove_after_exit: Option<PathBuf>,
}

/// Undoes install, keeping the data next to database.
pub async fn uninstall(database: &Path) -> Result<Uninstalled, String> {
    let layout = Layout::for_this_user()
        .ok_or("Windows didn't say where this user's AppData folders are, so there's nothing to find.")?;
    refuse_while_running()?;
    let mut done = Vec::new();
    let mut remove_after_exit = None;
    let schedule = ScanSchedule::task_scheduler(&layout.exe(), database);
    if let Ok(Some(_)) = schedule.interval_hours().await {
        schedule.remove().await?;
        done.push("Removed the job scan's task.".to_string());
    }
    match std::fs::remove_file(&layout.shortcut) {
        Ok(()) => done.push("Removed Recruiter from the Start menu.".to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("The Start menu's shortcut couldn't be removed: {e}")),
    }
    if system::remove_uninstall_entry().await? {
        done.push("Removed Recruiter from Windows' installed apps.".to_string());
    }
    if layout.folder.exists() {
        let running = std::env::current_exe().ok();
        if running.is_some_and(|running| running.starts_with(&layout.folder) || same_file(&running, &layout.exe())) {
            done.push(format!("{} goes once this window closes.", layout.folder.display()));
            remove_after_exit = Some(layout.folder.clone());
        } else {
            std::fs::remove_dir_all(&layout.folder)
                .map_err(|e| format!("{} couldn't be removed: {e}", layout.folder.display()))?;
            done.push(format!("Removed {}.", layout.folder.display()));
        }
    }
    let data = database.parent().unwrap_or(Path::new("."));
    done.push(format!(
        "Your data stays in {}: the database, the models and the Jobnet extension. The API keys stay in Windows Credential Manager, under Recruiter. Delete them by hand if you don't want them.",
        data.display()
    ));
    Ok(Uninstalled {
        done,
        remove_after_exit,
    })
}

/// Deletes folder a few seconds after this program has ended, from a cmd.exe of its own. Call
/// it as the last thing before exiting, after any wait for Enter: the wait is counted from here.
pub fn remove_after_exit(folder: &Path) -> Result<(), String> {
    system::remove_folder_after_exit(folder)
}

/// Copies running to the install folder, saying to close Recruiter when it is running (Windows
/// won't replace a running program's file).
async fn copy_program(running: &Path, layout: &Layout) -> Result<(), String> {
    std::fs::create_dir_all(&layout.folder)
        .map_err(|e| format!("{} couldn't be made: {e}", layout.folder.display()))?;
    if layout.exe().exists() {
        refuse_while_running()?;
    }
    tokio::fs::copy(running, layout.exe()).await.map(|_| ()).map_err(|e| {
        format!(
            "Recruiter couldn't be copied to {}: {e}. If it is running, close its window and install again.",
            layout.exe().display()
        )
    })
}

/// Fails when Recruiter answers on its port: its program file can't be replaced or removed then.
fn refuse_while_running() -> Result<(), String> {
    /// How long to wait for a running Recruiter to answer, in milliseconds.
    const ANSWER_WAIT_MS: u64 = 300;
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], crate::server::DEFAULT_PORT));
    let answers =
        std::net::TcpStream::connect_timeout(&address, std::time::Duration::from_millis(ANSWER_WAIT_MS)).is_ok();
    if answers {
        return Err(format!(
            "Recruiter is running (it answers on port {}). Close its window, then try again.",
            crate::server::DEFAULT_PORT
        ));
    }
    Ok(())
}

/// Whether a and b are the same file; false when either isn't there.
fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// The parts that run Windows' own tools.
#[cfg(windows)]
mod system {
    use std::os::windows::process::CommandExt;
    use std::path::Path;

    use super::{Layout, RegValue, UNINSTALL_KEY, clean_up_line, shortcut_script};

    /// A process without a window of its own (CREATE_NO_WINDOW).
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    /// A process that isn't attached to this console (DETACHED_PROCESS).
    const DETACHED_PROCESS: u32 = 0x0000_0008;

    pub async fn make_shortcut(layout: &Layout, arguments: &[String]) -> Result<(), String> {
        if let Some(folder) = layout.shortcut.parent() {
            std::fs::create_dir_all(folder).map_err(|e| format!("The Start menu's folder couldn't be made: {e}"))?;
        }
        let script = shortcut_script(layout, arguments);
        run(
            "powershell.exe",
            &["-NoProfile", "-NonInteractive", "-Command", &script],
        )
        .await
        .map(|_| ())
        .map_err(|why| format!("The Start menu's shortcut couldn't be made: {why}"))
    }

    pub async fn write_uninstall_entry(values: &[RegValue]) -> Result<(), String> {
        for value in values {
            run(
                "reg.exe",
                &[
                    "add",
                    UNINSTALL_KEY,
                    "/v",
                    value.name,
                    "/t",
                    value.kind,
                    "/d",
                    &value.data,
                    "/f",
                ],
            )
            .await
            .map_err(|why| format!("Windows' installed apps list couldn't be written: {why}"))?;
        }
        Ok(())
    }

    /// Removes the installed apps list's key; false when there was none.
    pub async fn remove_uninstall_entry() -> Result<bool, String> {
        if run("reg.exe", &["query", UNINSTALL_KEY]).await.is_err() {
            return Ok(false);
        }
        run("reg.exe", &["delete", UNINSTALL_KEY, "/f"])
            .await
            .map(|_| true)
            .map_err(|why| format!("Recruiter couldn't be removed from Windows' installed apps: {why}"))
    }

    /// Deletes folder a few seconds after this program has ended, from a cmd.exe of its own:
    /// Windows won't delete a running program's file.
    pub fn remove_folder_after_exit(folder: &Path) -> Result<(), String> {
        std::process::Command::new("cmd.exe")
            .raw_arg(clean_up_line(folder))
            .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("{} couldn't be removed: {e}", folder.display()))
    }

    /// Runs program with args, without a window: its output when it succeeded, else why not.
    async fn run(program: &str, args: &[&str]) -> Result<String, String> {
        let output = tokio::process::Command::new(program)
            .args(args)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .await
            .map_err(|e| format!("{program} couldn't run: {e}"))?;
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
        }
        let mut why = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if why.is_empty() {
            why = String::from_utf8_lossy(&output.stdout).trim().to_string();
        }
        Err(format!("{program} said: {why}"))
    }
}

/// Elsewhere there's no installer yet: macOS and Linux come next.
#[cfg(not(windows))]
mod system {
    use std::path::Path;

    use super::{Layout, RegValue};

    const NOT_HERE: &str = "Installing is only for Windows so far. On this computer, run the program where it is.";

    pub async fn make_shortcut(_: &Layout, _: &[String]) -> Result<(), String> {
        Err(NOT_HERE.to_string())
    }

    pub async fn write_uninstall_entry(_: &[RegValue]) -> Result<(), String> {
        Err(NOT_HERE.to_string())
    }

    pub async fn remove_uninstall_entry() -> Result<bool, String> {
        Err(NOT_HERE.to_string())
    }

    pub fn remove_folder_after_exit(_: &Path) -> Result<(), String> {
        Err(NOT_HERE.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> Layout {
        Layout::under(
            Path::new(r"C:\Users\me\AppData\Local"),
            Path::new(r"C:\Users\me\AppData\Roaming"),
        )
    }

    #[test]
    fn the_setup_name_starts_the_installer() {
        assert!(is_setup_name(Path::new(r"C:\Users\me\Downloads\Recruiter-setup.exe")));
        assert!(is_setup_name(Path::new("recruiter-SETUP")));
        assert!(!is_setup_name(Path::new(r"C:\Apps\recruiter.exe")));
        assert!(!is_setup_name(Path::new(r"C:\setup\recruiter.exe")));
    }

    #[test]
    fn the_uninstall_entry_runs_the_installed_copy() {
        let values = uninstall_values(&layout(), 43 * 1024 * 1024 + 1);
        let value = |name: &str| {
            values
                .iter()
                .find(|value| value.name == name)
                .map(|value| (value.kind, value.data.clone()))
        };
        // On Linux the joins use /, so the paths are compared as the layout makes them.
        let exe = layout().exe().display().to_string();
        assert_eq!(
            value("UninstallString"),
            Some(("REG_SZ", format!(r#""{exe}" uninstall"#)))
        );
        assert_eq!(value("DisplayName"), Some(("REG_SZ", "Recruiter".to_string())));
        assert_eq!(
            value("EstimatedSize"),
            Some(("REG_DWORD", (43 * 1024 + 1).to_string())),
            "in KB, rounded up"
        );
    }

    #[test]
    fn the_shortcut_script_quotes_paths_as_they_are() {
        let mut odd = layout();
        odd.folder = PathBuf::from(r"C:\Users\O'Brien\AppData\Local\Programs\Recruiter");
        let script = shortcut_script(&odd, &arguments_for(Some(Path::new(r"D:\My data\recruiter.sqlite"))));
        assert!(
            script.contains(r#"$shortcut.Arguments = '--database "D:\My data\recruiter.sqlite"'"#),
            "{script}"
        );
        assert!(shortcut_script(&odd, &[]).contains("$shortcut.Arguments = ''"));
        assert!(
            script.contains(r"$shortcut.WorkingDirectory = 'C:\Users\O''Brien\AppData\Local\Programs\Recruiter'"),
            "{script}"
        );
        assert!(script.ends_with("$shortcut.Save()"), "{script}");
    }

    #[test]
    fn the_clean_up_waits_then_deletes_the_folder() {
        assert_eq!(
            clean_up_line(Path::new(r"C:\Programs\Recruiter")),
            r#"/c ping -n 3 127.0.0.1 > nul & rmdir /s /q "C:\Programs\Recruiter""#
        );
    }
}
