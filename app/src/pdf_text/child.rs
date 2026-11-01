//! Reading a PDF in a child process (`recruiter read-pdf`), so a file that still exhausts memory
//! or the stack, or never finishes, ends only that process and not the app. The child reads the
//! PDF's bytes on stdin and prints its text on stdout, or why it couldn't on stderr.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

/// The subcommand the child runs.
pub const SUBCOMMAND: &str = "read-pdf";

/// The child's exit code when the file isn't a PDF it can read (its stderr says why).
pub const UNREADABLE: u8 = 2;

/// How long a PDF may take to read. A CV takes well under a second.
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// Windows' CREATE_NO_WINDOW process flag: no console window flashes up for the child.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Why the child gave no text.
#[derive(Debug, PartialEq)]
pub enum Failed {
    /// The file isn't a PDF the reader can read, as it said.
    Unreadable(String),
    /// The child crashed, ran out of memory or time, or couldn't be started.
    Stopped(String),
}

/// The child's side: the PDF on stdin, its text on stdout. Exits UNREADABLE with why on stderr.
pub fn run() -> std::process::ExitCode {
    use std::io::{Read, Write};
    let mut bytes = Vec::new();
    if let Err(e) = std::io::stdin().read_to_end(&mut bytes) {
        eprint!("the file couldn't be read: {e}");
        return std::process::ExitCode::from(UNREADABLE);
    }
    match super::text_of(bytes) {
        Ok(text) => match std::io::stdout().write_all(text.as_bytes()) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(_) => std::process::ExitCode::FAILURE,
        },
        Err(why) => {
            eprint!("{why}");
            std::process::ExitCode::from(UNREADABLE)
        }
    }
}

/// The PDF's text, read by `program read-pdf`.
pub async fn text_of(program: &Path, bytes: Vec<u8>) -> Result<String, Failed> {
    let mut command = Command::new(program);
    command
        .arg(SUBCOMMAND)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    let mut child = command
        .spawn()
        .map_err(|e| Failed::Stopped(format!("the PDF reader couldn't start: {e}")))?;
    let (Some(mut stdin), Some(mut stdout), Some(mut stderr)) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        return Err(Failed::Stopped("the PDF reader's pipes weren't open".to_string()));
    };
    let talk = async {
        // The bytes go in while the text comes out, so neither pipe fills up and waits.
        let write = async move {
            // A child that stopped early closes its stdin; what it says then is the answer.
            let _ = stdin.write_all(&bytes).await;
        };
        let (mut text, mut why) = (Vec::new(), Vec::new());
        let (_, out, err) = tokio::join!(write, stdout.read_to_end(&mut text), stderr.read_to_end(&mut why));
        let status = child.wait().await;
        (status, out.and(err), text, why)
    };
    let Ok((status, read, text, why)) = tokio::time::timeout(TIMEOUT, talk).await else {
        // Dropping the child (kill_on_drop) ends it.
        return Err(Failed::Stopped(format!(
            "reading it took over {} seconds",
            TIMEOUT.as_secs()
        )));
    };
    let status = status.map_err(|e| Failed::Stopped(format!("the PDF reader didn't finish: {e}")))?;
    read.map_err(|e| Failed::Stopped(format!("the PDF reader's answer couldn't be read: {e}")))?;
    match status.code() {
        Some(0) => Ok(String::from_utf8_lossy(&text).into_owned()),
        Some(code) if code == i32::from(UNREADABLE) => {
            Err(Failed::Unreadable(String::from_utf8_lossy(&why).into_owned()))
        }
        _ => Err(Failed::Stopped(format!("the PDF reader stopped ({status})"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_reader_that_cant_start_is_said_not_crashed_on() {
        let missing = Path::new("no-such-recruiter-program");
        match text_of(missing, b"%PDF-1.4".to_vec()).await {
            Err(Failed::Stopped(why)) => assert!(why.contains("couldn't start"), "{why}"),
            other => panic!("{other:?}"),
        }
    }
}
