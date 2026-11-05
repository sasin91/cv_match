//! Runs whisper-cli on a 16 kHz mono WAV (recordings::audio makes one) and reads the transcript it
//! writes as JSON: its timed lines and the language it heard.

use std::path::Path;

use serde::Deserialize;

use crate::task_manager::running_work;

/// whisper.cpp's command-line program.
#[cfg(windows)]
pub const PROGRAM_FILE: &str = "whisper-cli.exe";
#[cfg(not(windows))]
pub const PROGRAM_FILE: &str = "whisper-cli";

/// The share of the processor's cores whisper-cli computes on (one in this many), so the computer
/// stays usable while a recording is transcribed on the CPU.
const CORES_SHARE: usize = 2;

/// The language whisper-cli is asked to hear: whichever it detects (Danish or English, mostly).
const DETECT_LANGUAGE: &str = "auto";

/// The lines of output kept for the message when whisper-cli fails.
const OUTPUT_TAIL_LINES: usize = 12;

/// One timed line of a transcript.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub start_time_ms: i64,
    pub end_time_ms: i64,
    pub text: String,
}

/// What whisper-cli heard.
#[derive(Debug, Clone, PartialEq)]
pub struct Heard {
    /// The language it heard, as a short code ("da", "en"); "" when it didn't say.
    pub language: String,
    pub segments: Vec<Segment>,
}

impl Heard {
    /// The whole transcript: its lines, one per line.
    pub fn transcript(&self) -> String {
        self.segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// whisper-cli's JSON output (-oj), as much of it as is read.
#[derive(Debug, Deserialize)]
struct Output {
    #[serde(default)]
    result: OutputResult,
    #[serde(default)]
    transcription: Vec<OutputLine>,
}

#[derive(Debug, Default, Deserialize)]
struct OutputResult {
    #[serde(default)]
    language: String,
}

#[derive(Debug, Deserialize)]
struct OutputLine {
    offsets: OutputOffsets,
    text: String,
}

/// Where a line starts and ends, in milliseconds from the start.
#[derive(Debug, Deserialize)]
struct OutputOffsets {
    from: i64,
    to: i64,
}

/// Reads whisper-cli's JSON output; lines with nothing said are left out.
pub fn read_output(json: &str) -> Result<Heard, String> {
    let output: Output =
        serde_json::from_str(json).map_err(|e| format!("whisper-cli's transcript couldn't be read: {e}"))?;
    let segments = output
        .transcription
        .into_iter()
        .map(|line| Segment {
            start_time_ms: line.offsets.from,
            end_time_ms: line.offsets.to,
            text: line.text.trim().to_string(),
        })
        .filter(|segment| !segment.text.is_empty())
        .collect();
    Ok(Heard {
        language: output.result.language,
        segments,
    })
}

/// Transcribes wav (16 kHz mono) with model, through program; its JSON is written next to wav
/// and deleted after.
pub async fn transcribe(program: &Path, model: &Path, wav: &Path) -> Result<Heard, String> {
    let output_base = wav.with_extension("");
    let json_path = wav.with_extension("json");
    let mut command = tokio::process::Command::new(program);
    command
        .arg("--model")
        .arg(model)
        .arg("--file")
        .arg(wav)
        .args(["--language", DETECT_LANGUAGE])
        .args(["--threads", &background_threads().to_string()])
        .arg("--output-json")
        .arg("--output-file")
        .arg(&output_base)
        .arg("--no-prints")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let child = crate::no_window::in_background(&mut command)
        .spawn()
        .map_err(|e| format!("{} couldn't start: {e}", program.display()))?;
    running_work::set_process(child.id());
    let finished = child.wait_with_output().await;
    running_work::set_process(None);
    let finished = finished.map_err(|e| format!("whisper-cli stopped: {e}"))?;
    if !finished.status.success() {
        return Err(format!(
            "whisper-cli failed ({}): {}",
            finished.status,
            tail(&String::from_utf8_lossy(&finished.stderr))
        ));
    }
    let json = std::fs::read_to_string(&json_path).map_err(|e| format!("whisper-cli wrote no transcript: {e}"))?;
    if let Err(e) = std::fs::remove_file(&json_path) {
        crate::log_line!("whisper: {} couldn't be deleted: {e}", json_path.display());
    }
    read_output(&json)
}

/// The threads whisper-cli computes on: CORES_SHARE of the processor's, at least one.
fn background_threads() -> usize {
    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    (cores / CORES_SHARE).max(1)
}

/// The last OUTPUT_TAIL_LINES lines of output.
fn tail(output: &str) -> String {
    let lines: Vec<&str> = output.lines().collect();
    lines[lines.len().saturating_sub(OUTPUT_TAIL_LINES)..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_transcript_keeps_each_timed_line_and_the_language() {
        let json = r#"{
            "systeminfo": "AVX = 1",
            "result": { "language": "da" },
            "transcription": [
                { "timestamps": { "from": "00:00:00,000", "to": "00:00:02,500" },
                  "offsets": { "from": 0, "to": 2500 }, "text": " Hej, det er Mette fra Acme." },
                { "timestamps": { "from": "00:00:02,500", "to": "00:00:03,000" },
                  "offsets": { "from": 2500, "to": 3000 }, "text": " " },
                { "timestamps": { "from": "00:00:03,000", "to": "00:00:06,000" },
                  "offsets": { "from": 3000, "to": 6000 }, "text": " Vi vil gerne have dig til samtale." }
            ]
        }"#;
        let heard = read_output(json).unwrap();
        assert_eq!(heard.language, "da");
        assert_eq!(heard.segments.len(), 2);
        assert_eq!(heard.segments[1].start_time_ms, 3000);
        assert_eq!(
            heard.transcript(),
            "Hej, det er Mette fra Acme.\nVi vil gerne have dig til samtale."
        );
    }

    #[test]
    fn output_that_isnt_json_says_so() {
        assert!(read_output("whisper_init: failed").is_err());
    }
}
