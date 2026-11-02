//! The app's developer commands, run through the aliases in app/.cargo/config.toml.
//!
//!   cargo dev [jobfinder options]
//!       builds the app and starts it with --dev under watchexec, which rebuilds and restarts it
//!       on every change to its code, templates, queries, assets or extension. The window opens
//!       again with each restart, and a web page open in the browser reloads itself
//!       (src/dev_reload.rs). With --no-browser only the server runs, for work on the pages.
//!       Ctrl+C stops it.
//!
//!   cargo dev runs the app through `xtask run-app`, not cargo directly: on Windows, an app that
//!   crashes (a stack overflow, an access violation) ends with an exception code, and watchexec
//!   panics on one ("not implemented", watchexec-events' into_exitstatus) instead of waiting for
//!   the next change. run-app says which exception it was and ends with a plain error code.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// The command watchexec runs: the app's debug build, through run_app().
const RUN_APP: &str = "run-app";

/// What run_app() ends with when the app ended with an exception: an error, as watchexec takes it.
const CRASHED_EXIT_CODE: u8 = 1;

/// The first NTSTATUS code of the error kind (STATUS_SEVERITY_ERROR in the top two bits): a
/// Windows process that ends with a code from here up was ended by an exception.
const FIRST_EXCEPTION_CODE: u32 = 0xC000_0000;

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
        Some((command, rest)) if command == RUN_APP => run_app(rest),
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
    let this_program = match std::env::current_exe() {
        Ok(path) => path,
        Err(e) => {
            eprintln!("Couldn't find cargo dev's own program: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!("Building and starting JobFinder; its window opens when it is built.");
    let status = Command::new("watchexec")
        .current_dir(app_folder())
        .args(watchexec_args(&this_program, app_args))
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

/// watchexec's arguments: restart on any change to WATCHED, running the app through this
/// program's run-app (no shell, so the options reach it as they are).
fn watchexec_args(this_program: &Path, app_args: &[String]) -> Vec<String> {
    let mut args = vec!["--restart".to_string(), "--shell=none".to_string()];
    for path in WATCHED {
        args.extend(["--watch".to_string(), path.to_string()]);
    }
    args.extend([
        "--".to_string(),
        this_program.display().to_string(),
        RUN_APP.to_string(),
    ]);
    args.extend(app_args.iter().cloned());
    args
}

/// Builds and runs the app's debug build with --dev (its window, unless the options say
/// --no-browser or --browser) and the given options, and ends as it ended; an exception (a
/// crash on Windows) is said, and ends as an error code watchexec can take.
fn run_app(app_args: &[String]) -> ExitCode {
    let status = Command::new("cargo")
        .current_dir(app_folder())
        .args(["run", "--", "--dev"])
        .args(app_args)
        .status();
    let code = match status {
        Ok(status) => match status.code() {
            Some(code) => code,
            // Ended by a signal (Unix): cargo has said so.
            None => return ExitCode::FAILURE,
        },
        Err(e) => {
            eprintln!("Couldn't start cargo: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Some(exception) = exception_of(code) {
        eprintln!("JobFinder crashed: {exception}. It starts again on the next change.");
        return ExitCode::from(CRASHED_EXIT_CODE);
    }
    // An exit code is 0 to 255 on Unix; on Windows any other is an error too.
    ExitCode::from(u8::try_from(code).unwrap_or(CRASHED_EXIT_CODE))
}

/// The Windows exception an exit code says the program ended by, in words; None for an exit
/// code a program ends with itself.
fn exception_of(code: i32) -> Option<String> {
    // The code's 32 bits as Windows gives them, e.g. 0xC00000FD.
    let status = code as u32;
    if !cfg!(windows) || status < FIRST_EXCEPTION_CODE {
        return None;
    }
    Some(match exception_name(status) {
        Some(name) => format!("{name} (exception 0x{status:08X})"),
        None => format!("exception 0x{status:08X}"),
    })
}

/// The name of a common Windows exception code (ntstatus.h).
fn exception_name(status: u32) -> Option<&'static str> {
    Some(match status {
        0xC000_0005 => "access violation",
        0xC000_00FD => "stack overflow",
        0xC000_0409 => "stack buffer overrun, or a Rust abort",
        0xC000_001D => "illegal instruction",
        0xC000_0094 => "integer divided by zero",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn the_app_options_go_on_after_run_app() {
        let args = watchexec_args(Path::new("xtask"), &strings(&["--database", "dev.sqlite"]));
        let tail = &args[args.len() - 5..];
        assert_eq!(tail, strings(&["--", "xtask", RUN_APP, "--database", "dev.sqlite"]));
    }

    #[test]
    fn a_crash_on_windows_is_said_in_words_and_an_exit_code_is_not() {
        let stack_overflow = 0xC000_00FD_u32 as i32;
        if cfg!(windows) {
            assert_eq!(
                exception_of(stack_overflow).as_deref(),
                Some("stack overflow (exception 0xC00000FD)")
            );
        } else {
            assert_eq!(exception_of(stack_overflow), None);
        }
        assert_eq!(exception_of(101), None);
        assert_eq!(exception_of(0), None);
    }
}
