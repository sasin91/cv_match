//! The app's developer commands, run through the aliases in app/.cargo/config.toml.
//!
//!   cargo dev [jobfinder options]
//!       builds the app and starts it with --dev under watchexec, which rebuilds and restarts it
//!       on every change to its code, templates, queries, assets or extension. The window opens
//!       again with each restart, and a web page open in the browser reloads itself
//!       (src/dev_reload.rs). With --no-browser only the server runs, for work on the pages.
//!       Ctrl+C stops it.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "usage: cargo dev [jobfinder options, such as --database <file> or --no-browser]";

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

fn dev(app_args: &[String]) -> ExitCode {
    if Command::new("watchexec").arg("--version").output().is_err() {
        eprintln!("cargo dev needs watchexec. Install it once with: cargo install watchexec-cli --locked");
        return ExitCode::FAILURE;
    }
    println!("Building and starting JobFinder; its window opens when it is built.");
    let status = Command::new("watchexec")
        .current_dir(app_folder())
        .args(watchexec_args(app_args))
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

/// watchexec's arguments: restart on any change to WATCHED, running the app's debug build with
/// --dev (its window, unless the options say --no-browser or --browser) and the given options (no shell, so they reach it as they are).
fn watchexec_args(app_args: &[String]) -> Vec<String> {
    let mut args = vec!["--restart".to_string(), "--shell=none".to_string()];
    for path in WATCHED {
        args.extend(["--watch".to_string(), path.to_string()]);
    }
    args.extend(["--", "cargo", "run", "--", "--dev"].map(str::to_string));
    args.extend(app_args.iter().cloned());
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn the_app_options_go_on_after_dev() {
        let args = watchexec_args(&strings(&["--database", "dev.sqlite"]));
        let tail = &args[args.len() - 3..];
        assert_eq!(tail, strings(&["--dev", "--database", "dev.sqlite"]));
    }
}
