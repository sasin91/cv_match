//! Pages that show their job post with JavaScript, read in the computer's own browser: Edge
//! (always there on Windows 11) or Chrome, or Chromium on Linux, started headless for one page
//! with a fresh profile of its own, so it never touches the user's. It runs the page's
//! JavaScript for a few seconds and prints the page to a PDF, whose text the app's PDF reader
//! reads. JOBFINDER_BROWSER names another browser's program.
//!
//! The link itself is checked by page_reader before the browser gets it; what the page loads
//! after that, the browser fetches as it would for the user.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::pdf_text::child;
use crate::task_manager::running_work::{self, Stopped, WorkKind};

/// The environment variable that names the browser's program, instead of looking for one.
const BROWSER_VARIABLE: &str = "JOBFINDER_BROWSER";

/// How long the page's JavaScript may run before its HTML is taken, in milliseconds of the
/// browser's own clock (--virtual-time-budget), which skips ahead while the page waits.
const SCRIPT_BUDGET_MS: u64 = 10_000;

/// How long the browser may take in all, in seconds, before it is stopped.
const BROWSER_SECONDS: u64 = 45;

/// Most bytes of the PDF the browser prints that are read.
const MAX_PDF: usize = 20_000_000;

/// The PDF's name in the run's profile folder.
const PDF_FILE: &str = "page.pdf";

/// Where Windows keeps the browsers, under Program Files or the user's local app data.
#[cfg(windows)]
const WINDOWS_BROWSERS: [&str; 2] = [
    r"Microsoft\Edge\Application\msedge.exe",
    r"Google\Chrome\Application\chrome.exe",
];
/// The folders WINDOWS_BROWSERS are looked for under, as environment variables.
#[cfg(windows)]
const WINDOWS_FOLDERS: [&str; 3] = ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"];

/// The browsers' programs looked for on the PATH elsewhere.
#[cfg(not(windows))]
const PATH_BROWSERS: [&str; 5] = [
    "microsoft-edge",
    "google-chrome",
    "google-chrome-stable",
    "chromium",
    "chromium-browser",
];

/// The text of the page at url as the browser shows it after its JavaScript ran, or None when
/// there is no browser or it failed (logged); Err when it was stopped in the task manager, where
/// it is listed while it reads. The browser prints the page to a PDF, which the app's PDF reader
/// (pdf_reader, `jobfinder read-pdf`) reads: Edge on Windows prints nothing for --dump-dom, but
/// --print-to-pdf works everywhere.
pub async fn render(url: &str, pdf_reader: &Path) -> Result<Option<String>, Stopped> {
    let Some(browser) = find() else {
        crate::log_line!("browser: no Edge, Chrome or Chromium found to read {url}");
        return Ok(None);
    };
    let reading = async {
        let pdf = print_pdf(&browser, url).await?;
        // The browser has ended; the PDF reader is listed under this row with its own process.
        running_work::set_process(None);
        child::text_of(pdf_reader, pdf)
            .await
            .map_err(|e| format!("its PDF couldn't be read: {}", failure(&e)))
    };
    match running_work::part(WorkKind::Browser, format!("Reading {url}"), reading).await? {
        Ok(text) => Ok(Some(text)),
        // Its PDF reader was stopped.
        Err(why) if why.contains(Stopped::SAID) => Err(Stopped),
        Err(why) => {
            crate::log_line!("browser: {} couldn't read {url}: {why}", browser.display());
            Ok(None)
        }
    }
}

/// Why the PDF reader gave no text, in words.
fn failure(failed: &child::Failed) -> &str {
    match failed {
        child::Failed::Unreadable(why) | child::Failed::Stopped(why) => why,
    }
}

/// The browser's program: JOBFINDER_BROWSER, else the first one found.
pub fn find() -> Option<PathBuf> {
    if let Some(named) = std::env::var_os(BROWSER_VARIABLE).filter(|named| !named.is_empty()) {
        return Some(PathBuf::from(named));
    }
    installed()
}

#[cfg(windows)]
fn installed() -> Option<PathBuf> {
    WINDOWS_BROWSERS.iter().find_map(|browser| {
        WINDOWS_FOLDERS
            .iter()
            .filter_map(std::env::var_os)
            .map(|folder| Path::new(&folder).join(browser))
            .find(|program| program.is_file())
    })
}

#[cfg(not(windows))]
fn installed() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    PATH_BROWSERS.iter().find_map(|browser| {
        std::env::split_paths(&path)
            .map(|folder| folder.join(browser))
            .find(|program| program.is_file())
    })
}

/// The arguments that print url, once its JavaScript ran, to the PDF pdf from a headless browser
/// with the profile in profile.
fn arguments(url: &str, profile: &Path, pdf: &Path) -> Vec<String> {
    vec![
        "--headless=new".to_string(),
        "--disable-gpu".to_string(),
        "--no-first-run".to_string(),
        "--no-default-browser-check".to_string(),
        "--disable-extensions".to_string(),
        "--disable-sync".to_string(),
        "--mute-audio".to_string(),
        format!("--user-data-dir={}", profile.display()),
        format!("--virtual-time-budget={SCRIPT_BUDGET_MS}"),
        "--no-pdf-header-footer".to_string(),
        format!("--print-to-pdf={}", pdf.display()),
        url.to_string(),
    ]
}

/// Runs browser on url with a fresh profile, removed after with the PDF it printed in it, and
/// takes the PDF.
async fn print_pdf(browser: &Path, url: &str) -> Result<Vec<u8>, String> {
    let profile = fresh_profile();
    let pdf = profile.join(PDF_FILE);
    let printed = run_printing(browser, url, &profile, &pdf).await;
    // The profile goes whatever happened; a browser still holding a file of it leaves it in the
    // temp folder, which Windows empties.
    let _ = tokio::fs::remove_dir_all(&profile).await;
    printed
}

async fn run_printing(browser: &Path, url: &str, profile: &Path, pdf: &Path) -> Result<Vec<u8>, String> {
    tokio::fs::create_dir_all(profile)
        .await
        .map_err(|e| format!("its profile folder couldn't be made: {e}"))?;
    let mut command = tokio::process::Command::new(browser);
    command
        .args(arguments(url, profile, pdf))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    crate::no_window::in_background(&mut command);
    let mut child = command.spawn().map_err(|e| format!("it didn't start: {e}"))?;
    running_work::set_process(child.id());
    let status = tokio::time::timeout(Duration::from_secs(BROWSER_SECONDS), child.wait())
        .await
        .map_err(|_| format!("it took more than {BROWSER_SECONDS} seconds"))?
        .map_err(|e| format!("it didn't start: {e}"))?;
    if !status.success() {
        return Err(format!("it stopped with {status}"));
    }
    let printed = tokio::fs::read(pdf)
        .await
        .map_err(|e| format!("it printed no PDF: {e}"))?;
    if printed.len() > MAX_PDF {
        return Err("the page is too large".to_string());
    }
    Ok(printed)
}

/// Browser runs started by this process, which names each run's profile: only ever counts up.
static RUNS: AtomicU64 = AtomicU64::new(0);

/// A profile folder of its own for one browser run, in the temp folder.
fn fresh_profile() -> PathBuf {
    let run = RUNS.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("jobfinder-browser-{}-{run}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_browser_runs_headless_on_its_own_profile() {
        let arguments = arguments("https://example.com/job", Path::new("profile"), Path::new("page.pdf"));
        assert_eq!(arguments.first().map(String::as_str), Some("--headless=new"));
        assert!(arguments.contains(&"--user-data-dir=profile".to_string()));
        assert!(arguments.contains(&"--print-to-pdf=page.pdf".to_string()));
        assert_eq!(arguments.last().map(String::as_str), Some("https://example.com/job"));
    }

    #[test]
    fn each_run_gets_a_profile_of_its_own() {
        assert_ne!(fresh_profile(), fresh_profile());
    }
}
