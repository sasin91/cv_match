//! JobFinder's own signed-in browser: Edge (or Chrome, or Chromium) on a profile of the app's
//! own, in the data folder, never the user's. The user signs in to LinkedIn and Indeed in it once
//! (open_sign_in shows it as a window); the job scan then reads those sites' search pages and
//! posts in it headless, one page at a time and PAUSE_BETWEEN_PAGES apart, through its DevTools
//! (cdp), which give the page's HTML once its JavaScript has shown the posts.
//!
//! It does nothing to look like a person: a site that asks it to sign in again or to show it is
//! a person gets no more pages. The scan leaves that site alone (a file in PAUSED_FOLDER says
//! why) until the user opens the sign-in window again.
//!
//! The sites' terms forbid reading them with programs; the user chose this knowing that, for
//! their own job hunting at a few pages a scan.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::browser;
use super::cdp::{self, DevTools};

/// The folder under the data folder that holds the browser's profile and the paused sites.
pub const FOLDER: &str = "signed-in-browser";

/// The profile's folder in FOLDER.
const PROFILE_FOLDER: &str = "profile";

/// The folder in FOLDER with a file per paused site, named by its domain, saying why.
const PAUSED_FOLDER: &str = "paused";

/// The file the browser writes in its profile with the port its DevTools listen on.
const ACTIVE_PORT_FILE: &str = "DevToolsActivePort";

/// How long the browser may take to start and open its DevTools, in seconds.
const START_SECONDS: u64 = 20;

/// How long a page's JavaScript may take to show what is read from it, in seconds.
const SHOWN_SECONDS: u64 = 25;

/// How long one page may take in all, from starting the browser to closing it, in seconds.
const PAGE_SECONDS: u64 = 90;

/// How often the page is looked at while its JavaScript shows it, in milliseconds.
const LOOK_EVERY_MS: u64 = 1500;

/// How often the port file is looked for while the browser starts, in milliseconds.
const START_LOOK_EVERY_MS: u64 = 250;

/// How long the browser may take to close after it is asked to, in seconds, before it is ended.
const CLOSE_SECONDS: u64 = 5;

/// The least time between two pages read in the browser, in seconds: a site gets a page at a
/// time, at a person's pace.
pub const PAUSE_BETWEEN_PAGES: u64 = 10;

/// The browser's window size while headless: wide enough for the sites' desktop layout.
const WINDOW_SIZE: &str = "1280,1600";

/// What a site's page shows, as one of its readers (linkedin, indeed) sees it.
#[derive(Debug, Clone, PartialEq)]
pub enum Seen<T> {
    /// What was read.
    Shown(T),
    /// Not yet: its JavaScript may still be showing it.
    NotYet,
    /// The site asks the browser to sign in.
    SignIn,
    /// The site asks the browser to show it is a person (a CAPTCHA or a security check).
    Check,
}

/// The signed-in browser of one data folder.
pub struct SignedInBrowser {
    folder: PathBuf,
    /// When this process last read a page in it: held while a page is read, so the pages go one
    /// at a time and PAUSE_BETWEEN_PAGES apart.
    last_page: tokio::sync::Mutex<Option<Instant>>,
}

impl SignedInBrowser {
    /// The signed-in browser whose profile is in folder (FOLDER of the data folder).
    pub fn new(folder: PathBuf) -> SignedInBrowser {
        SignedInBrowser {
            folder,
            last_page: tokio::sync::Mutex::new(None),
        }
    }

    fn profile(&self) -> PathBuf {
        self.folder.join(PROFILE_FOLDER)
    }

    fn pause_file(&self, domain: &str) -> PathBuf {
        self.folder.join(PAUSED_FOLDER).join(format!("{domain}.txt"))
    }

    /// Opens the browser as a window on pages, to sign in to them; its paused sites are read
    /// again from now on. Err says why it couldn't open.
    pub fn open_sign_in(&self, pages: &[&str]) -> Result<(), String> {
        let program =
            browser::find().ok_or("There is no Edge, Chrome or Chromium on this computer to sign in with.")?;
        std::fs::create_dir_all(self.profile())
            .map_err(|e| format!("JobFinder's browser folder couldn't be made: {e}"))?;
        let _ = std::fs::remove_dir_all(self.folder.join(PAUSED_FOLDER));
        std::process::Command::new(&program)
            .args(window_arguments(&self.profile(), pages))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|e| {
                crate::log_line!("signed_in: {} didn't start: {e}", program.display());
                format!("{} didn't start: {e}", program.display())
            })
    }

    /// Why the scan leaves domain alone, when a page of it asked to sign in or for a check.
    pub fn pause_of(&self, domain: &str) -> Option<String> {
        std::fs::read_to_string(self.pause_file(domain)).ok()
    }

    /// Leaves domain alone from now on, saying why.
    fn pause(&self, domain: &str, why: &str) -> String {
        let today = chrono::Local::now().format("%-d %B %Y");
        let said = format!(
            "{why} ({today}). The scan leaves {domain} alone until you sign in again in JobFinder's browser (Job searches, “Sign in to job sites”)."
        );
        let file = self.pause_file(domain);
        let written = file
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&file, &said));
        if let Err(e) = written {
            crate::log_line!("signed_in: keeping {domain}'s pause in {} failed: {e}", file.display());
        }
        crate::log_line!("signed_in: {said}");
        said
    }

    /// What judge reads from url, a page of domain, once it shows it. Err says why not: the site
    /// is paused, asked to sign in or for a check (it is paused from now on), or didn't show it.
    pub async fn read<T>(&self, domain: &str, url: &str, judge: impl Fn(&str, &str) -> Seen<T>) -> Result<T, String> {
        if let Some(paused) = self.pause_of(domain) {
            return Err(paused);
        }
        let mut last_page = self.last_page.lock().await;
        if let Some(at) = *last_page {
            let pause = Duration::from_secs(PAUSE_BETWEEN_PAGES);
            tokio::time::sleep(pause.saturating_sub(at.elapsed())).await;
        }
        let seen = tokio::time::timeout(Duration::from_secs(PAGE_SECONDS), self.look(url, &judge)).await;
        *last_page = Some(Instant::now());
        let seen = seen.map_err(|_| format!("JobFinder's browser took more than {PAGE_SECONDS} seconds on {url}."))?;
        match seen? {
            Seen::Shown(read) => Ok(read),
            Seen::NotYet => Err(format!(
                "{url} didn't show what JobFinder reads in {SHOWN_SECONDS} seconds: the page may be gone, or the site changed."
            )),
            Seen::SignIn => Err(self.pause(domain, &format!("{domain} asked JobFinder's browser to sign in"))),
            Seen::Check => Err(self.pause(
                domain,
                &format!("{domain} asked JobFinder's browser to show it is a person"),
            )),
        }
    }

    /// Starts the browser headless, opens url and looks at it until judge sees something.
    async fn look<T>(&self, url: &str, judge: &impl Fn(&str, &str) -> Seen<T>) -> Result<Seen<T>, String> {
        let program = browser::find().ok_or("There is no Edge, Chrome or Chromium on this computer.")?;
        let profile = self.profile();
        tokio::fs::create_dir_all(&profile)
            .await
            .map_err(|e| format!("JobFinder's browser folder couldn't be made: {e}"))?;
        let port_file = profile.join(ACTIVE_PORT_FILE);
        let _ = tokio::fs::remove_file(&port_file).await;
        let mut command = tokio::process::Command::new(&program);
        command
            .args(headless_arguments(&profile))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        crate::no_window::hide(&mut command);
        let mut child = command
            .spawn()
            .map_err(|e| format!("{} didn't start: {e}", program.display()))?;
        let looked = match devtools_address(&port_file).await {
            Some(address) => look_at(&address, url, judge).await,
            None => Err(format!(
                "JobFinder's browser didn't start in {START_SECONDS} seconds. Is its sign-in window still open? Close it and scan again."
            )),
        };
        if tokio::time::timeout(Duration::from_secs(CLOSE_SECONDS), child.wait())
            .await
            .is_err()
        {
            let _ = child.kill().await;
        }
        looked
    }
}

/// The browser's DevTools address, once it has written its port file.
async fn devtools_address(port_file: &Path) -> Option<String> {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(START_SECONDS) {
        if let Some(address) = tokio::fs::read_to_string(port_file)
            .await
            .ok()
            .and_then(|text| cdp::address_of(&text))
        {
            return Some(address);
        }
        tokio::time::sleep(Duration::from_millis(START_LOOK_EVERY_MS)).await;
    }
    None
}

/// Opens url in the browser at address and looks at it until judge sees something, or
/// SHOWN_SECONDS have gone by; then closes the browser.
async fn look_at<T>(address: &str, url: &str, judge: &impl Fn(&str, &str) -> Seen<T>) -> Result<Seen<T>, String> {
    let mut devtools = DevTools::connect(address).await?;
    let seen = look_through(&mut devtools, url, judge).await;
    devtools.close_browser().await;
    seen
}

async fn look_through<T>(
    devtools: &mut DevTools,
    url: &str,
    judge: &impl Fn(&str, &str) -> Seen<T>,
) -> Result<Seen<T>, String> {
    devtools.open_page().await?;
    devtools.navigate(url).await?;
    let started = Instant::now();
    loop {
        tokio::time::sleep(Duration::from_millis(LOOK_EVERY_MS)).await;
        let (location, html) = devtools.location_and_html().await?;
        let seen = judge(&location, &html);
        if !seen.is_not_yet() || started.elapsed() >= Duration::from_secs(SHOWN_SECONDS) {
            return Ok(seen);
        }
    }
}

/// The arguments that start the browser headless on profile, its DevTools on a port it picks.
fn headless_arguments(profile: &Path) -> Vec<String> {
    vec![
        "--headless=new".to_string(),
        "--disable-gpu".to_string(),
        "--no-first-run".to_string(),
        "--no-default-browser-check".to_string(),
        "--disable-sync".to_string(),
        "--mute-audio".to_string(),
        format!("--window-size={WINDOW_SIZE}"),
        format!("--user-data-dir={}", profile.display()),
        "--remote-debugging-port=0".to_string(),
        "about:blank".to_string(),
    ]
}

/// The arguments that open the browser as a window on profile, showing pages.
fn window_arguments(profile: &Path, pages: &[&str]) -> Vec<String> {
    let mut arguments = vec![
        "--no-first-run".to_string(),
        "--no-default-browser-check".to_string(),
        format!("--user-data-dir={}", profile.display()),
    ];
    arguments.extend(pages.iter().map(|page| page.to_string()));
    arguments
}

impl<T> Seen<T> {
    /// Whether the page doesn't show anything to read yet.
    fn is_not_yet(&self) -> bool {
        matches!(self, Seen::NotYet)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headless_runs_on_the_apps_own_profile_with_devtools() {
        let arguments = headless_arguments(Path::new("profile"));
        assert_eq!(arguments.first().map(String::as_str), Some("--headless=new"));
        assert!(arguments.contains(&"--user-data-dir=profile".to_string()));
        assert!(arguments.contains(&"--remote-debugging-port=0".to_string()));
    }

    #[test]
    fn the_sign_in_window_is_not_headless() {
        let arguments = window_arguments(Path::new("profile"), &["https://www.linkedin.com/login"]);
        assert!(!arguments.iter().any(|argument| argument.starts_with("--headless")));
        assert_eq!(
            arguments.last().map(String::as_str),
            Some("https://www.linkedin.com/login")
        );
    }

    #[test]
    fn a_paused_site_says_why_until_signing_in_again() {
        let folder = std::env::temp_dir().join(format!("jobfinder-signed-in-test-{}", std::process::id()));
        let browser = SignedInBrowser::new(folder.clone());
        assert_eq!(browser.pause_of("linkedin.com"), None);
        let said = browser.pause("linkedin.com", "linkedin.com asked to sign in");
        assert_eq!(browser.pause_of("linkedin.com"), Some(said));
        assert_eq!(browser.pause_of("dk.indeed.com"), None);
        let _ = std::fs::remove_dir_all(&folder);
    }
}

#[cfg(test)]
mod in_a_browser {
    use super::*;

    /// Reads a page in a real browser (JOBFINDER_BROWSER): `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn a_page_is_read_once_its_javascript_shows_it() {
        let folder = std::env::temp_dir().join(format!("jobfinder-signed-in-live-{}", std::process::id()));
        let browser = SignedInBrowser::new(folder.clone());
        let page = "data:text/html,<body><script>setTimeout(()=>{document.body.innerHTML='<p id=x>shown</p>'},2000)</script></body>";
        let read = browser
            .read("example.com", page, |_, html| {
                if html.contains("<p id=\"x\">shown</p>") {
                    Seen::Shown(html.len())
                } else {
                    Seen::NotYet
                }
            })
            .await;
        let _ = std::fs::remove_dir_all(&folder);
        assert!(read.is_ok(), "{read:?}");
    }
}
