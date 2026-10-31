//! The app's developer commands, run through the aliases in app/.cargo/config.toml.
//!
//!   cargo dev [--port 8765] [more recruiter options]
//!       builds the app and starts it with --dev under watchexec, which rebuilds and restarts it
//!       on every change to its code, templates, queries, assets or extension; the open page
//!       then reloads itself (src/dev_reload.rs). Opens the match page once the first build
//!       answers. Ctrl+C stops it.

use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::thread;
use std::time::{Duration, Instant};

const USAGE: &str = "usage: cargo dev [--port <port>] [more recruiter options, such as --database <file>]";

/// The port the app listens on unless --port says otherwise (server::DEFAULT_PORT).
const DEFAULT_PORT: u16 = 8765;

/// How long to wait for the first build to answer before giving up on opening the page.
const FIRST_BUILD_WAIT: Duration = Duration::from_secs(15 * 60);

/// How often to try the app's port while the first build runs.
const PORT_CHECK_INTERVAL: Duration = Duration::from_secs(1);

/// What restarts the app when it changes, relative to app/.
const WATCHED: &[&str] = &[
    "src",
    "templates",
    "queries",
    "migrations",
    ".sqlx",
    "Cargo.toml",
    "../assets",
    "../extension",
    "../laya-server/src",
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.split_first() {
        Some((command, rest)) if command == "dev" => dev(rest),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}

fn dev(recruiter_args: &[String]) -> ExitCode {
    if Command::new("watchexec").arg("--version").output().is_err() {
        eprintln!("cargo dev needs watchexec. Install it once with: cargo install watchexec-cli --locked");
        return ExitCode::FAILURE;
    }
    let port = match port(recruiter_args) {
        Ok(port) => port,
        Err(why) => {
            eprintln!("{why}\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    let address = format!("http://127.0.0.1:{port}/match");
    println!("Building and starting Recruiter; {address} opens when it is up.");
    thread::spawn(move || open_when_up(port, &address));
    let status = Command::new("watchexec")
        .current_dir(app_folder())
        .args(watchexec_args(recruiter_args))
        .status();
    match status {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(status) => {
            eprintln!("watchexec stopped: {status}");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("Couldn't start watchexec: {e}");
            ExitCode::FAILURE
        }
    }
}

/// app/, where cargo and the watched paths are run from.
fn app_folder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// The port given with --port, or DEFAULT_PORT.
fn port(recruiter_args: &[String]) -> Result<u16, String> {
    let Some(at) = recruiter_args.iter().position(|arg| arg == "--port") else {
        return Ok(DEFAULT_PORT);
    };
    recruiter_args
        .get(at + 1)
        .and_then(|port| port.parse().ok())
        .ok_or_else(|| "--port needs a number".to_string())
}

/// watchexec's arguments: restart on any change to WATCHED, running the app's debug build with
/// --dev and the given options (no shell, so they reach it as they are).
fn watchexec_args(recruiter_args: &[String]) -> Vec<String> {
    let mut args = vec!["--restart".to_string(), "--shell=none".to_string()];
    for path in WATCHED {
        args.extend(["--watch".to_string(), path.to_string()]);
    }
    args.extend(["--", "cargo", "run", "--", "--dev", "--no-browser"].map(str::to_string));
    args.extend(recruiter_args.iter().cloned());
    args
}

/// Opens the match page in the default browser once the app first listens on its port; after
/// that the page reloads itself on each restart.
fn open_when_up(port: u16, address: &str) {
    let app = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let started = Instant::now();
    while started.elapsed() < FIRST_BUILD_WAIT {
        if TcpStream::connect_timeout(&app, PORT_CHECK_INTERVAL).is_ok() {
            if let Err(e) = open_browser(address) {
                println!("Couldn't open the browser ({e}); open {address} yourself.");
            }
            return;
        }
        thread::sleep(PORT_CHECK_INTERVAL);
    }
}

fn open_browser(address: &str) -> std::io::Result<()> {
    let mut command = if cfg!(windows) {
        let mut command = Command::new("cmd");
        // start's first quoted argument is the window title.
        command.args(["/C", "start", ""]);
        command
    } else if cfg!(target_os = "macos") {
        Command::new("open")
    } else {
        Command::new("xdg-open")
    };
    command.arg(address).spawn().map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn the_port_is_the_default_or_the_one_given() {
        assert_eq!(port(&[]), Ok(DEFAULT_PORT));
        assert_eq!(port(&strings(&["--database", "x", "--port", "8766"])), Ok(8766));
        assert!(port(&strings(&["--port"])).is_err());
    }

    #[test]
    fn the_app_options_go_on_after_dev() {
        let args = watchexec_args(&strings(&["--database", "dev.sqlite"]));
        let tail = &args[args.len() - 4..];
        assert_eq!(tail, strings(&["--dev", "--no-browser", "--database", "dev.sqlite"]));
    }
}
