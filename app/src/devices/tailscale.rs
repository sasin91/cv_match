//! Tailscale on this computer, through its own command line: whether it runs and the computer's
//! name on the tailnet (`tailscale status --json`), whether `tailscale serve` passes the device
//! port on to the app (`tailscale serve status --json`), and setting that up (`tailscale serve
//! --bg`). Never `funnel`, which would put the app on the open internet.

use std::time::Duration;

use serde_json::Value;

use crate::context::Context;

/// The command line's program when it is on PATH.
const TAILSCALE: &str = "tailscale";

/// Where Tailscale's installer puts its command line on Windows, for when it isn't on PATH.
#[cfg(windows)]
const TAILSCALE_ON_WINDOWS: &str = r"C:\Program Files\Tailscale\tailscale.exe";

/// How long a tailscale command may take before it is given up on.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

/// Tailscale's BackendState once it is signed in and connected.
const RUNNING: &str = "Running";

/// How Tailscale stands on this computer, for the Devices screen.
#[derive(Debug, Clone, PartialEq)]
pub enum TailscaleState {
    /// Its command line couldn't be run: not installed, most likely. Why, in words.
    Missing(String),
    /// Installed but not connected: its BackendState ("Stopped", "NeedsLogin").
    NotRunning(String),
    /// Connected, as name on the tailnet; serving says whether `tailscale serve` passes the
    /// device port on to the app.
    Running { name: String, serving: bool },
}

impl TailscaleState {
    /// The computer's tailnet name, when Tailscale runs.
    pub fn name(&self) -> Option<&str> {
        match self {
            TailscaleState::Running { name, .. } => Some(name),
            _ => None,
        }
    }
}

/// How Tailscale stands now, for the app's device port. The tailnet name is kept in the
/// context's Devices, as device_server checks the Host by it.
pub async fn state(context: &Context, port: u16, now: i64) -> TailscaleState {
    let state = read_state(port).await;
    context.devices.set_tailnet_name(state.name().map(str::to_string), now);
    state
}

async fn read_state(port: u16) -> TailscaleState {
    let status = match run(&["status", "--json"]).await {
        Ok(status) => status,
        Err(why) => return TailscaleState::Missing(why),
    };
    let (backend, name) = match status_of(&status) {
        Ok(read) => read,
        Err(why) => return TailscaleState::Missing(why),
    };
    let Some(name) = name.filter(|_| backend == RUNNING) else {
        return TailscaleState::NotRunning(backend);
    };
    let serving = match run(&["serve", "status", "--json"]).await {
        Ok(served) => serves(&served, &name, port),
        Err(why) => {
            crate::log_line!("devices: tailscale serve status: {why}");
            false
        }
    };
    TailscaleState::Running { name, serving }
}

/// Sets `tailscale serve` up to pass https://<name>:port on to the app at 127.0.0.1:port, in the
/// background, where it stays across restarts.
pub async fn serve(port: u16) -> Result<(), String> {
    let args = serve_args(port);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    run(&args).await.map(|_| ())
}

/// The command a user types to do what serve does.
pub fn serve_command(port: u16) -> String {
    format!("{TAILSCALE} {}", serve_args(port).join(" "))
}

/// What serve runs tailscale with.
fn serve_args(port: u16) -> [String; 4] {
    [
        "serve".to_string(),
        "--bg".to_string(),
        format!("--https={port}"),
        format!("http://127.0.0.1:{port}"),
    ]
}

/// BackendState and Self.DNSName (without its final dot) from `tailscale status --json`.
fn status_of(json: &str) -> Result<(String, Option<String>), String> {
    let status: Value = serde_json::from_str(json).map_err(|e| format!("Tailscale's status couldn't be read: {e}"))?;
    let backend = status["BackendState"].as_str().unwrap_or_default().to_string();
    let name = status["Self"]["DNSName"]
        .as_str()
        .map(|name| name.trim_end_matches('.').to_string())
        .filter(|name| !name.is_empty());
    Ok((backend, name))
}

/// Whether `tailscale serve status --json` says https://<name>:port is passed on to the app at
/// 127.0.0.1:port.
fn serves(json: &str, name: &str, port: u16) -> bool {
    let Ok(served) = serde_json::from_str::<Value>(json) else {
        return false;
    };
    let target = format!("http://127.0.0.1:{port}");
    served["Web"][format!("{name}:{port}")]["Handlers"]
        .as_object()
        .is_some_and(|handlers| handlers.values().any(|handler| handler["Proxy"] == target.as_str()))
}

/// Runs tailscale with args and answers what it printed; Err says why it couldn't, in words.
async fn run(args: &[&str]) -> Result<String, String> {
    let output = match output_of(TAILSCALE, args).await {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => on_windows_path(args).await,
        output => output,
    };
    let output = output.map_err(|e| format!("Tailscale's command line couldn't run: {e}. Is Tailscale installed?"))?;
    let printed = String::from_utf8_lossy(&output.stdout).to_string();
    if output.status.success() {
        return Ok(printed);
    }
    let said = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(format!("tailscale {} failed: {said}", args.join(" ")))
}

#[cfg(windows)]
async fn on_windows_path(args: &[&str]) -> std::io::Result<std::process::Output> {
    output_of(TAILSCALE_ON_WINDOWS, args).await
}

#[cfg(not(windows))]
async fn on_windows_path(_args: &[&str]) -> std::io::Result<std::process::Output> {
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "tailscale isn't on PATH",
    ))
}

async fn output_of(program: &str, args: &[&str]) -> std::io::Result<std::process::Output> {
    let mut command = tokio::process::Command::new(program);
    command.args(args).kill_on_drop(true);
    crate::no_window::hide(&mut command);
    match tokio::time::timeout(COMMAND_TIMEOUT, command.output()).await {
        Ok(output) => output,
        Err(_) => Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!("it didn't answer in {} seconds", COMMAND_TIMEOUT.as_secs()),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_status_gives_the_state_and_the_name_without_its_dot() {
        let status = r#"{"BackendState":"Running","Self":{"DNSName":"pc.tail1234.ts.net."}}"#;
        assert_eq!(
            status_of(status),
            Ok(("Running".to_string(), Some("pc.tail1234.ts.net".to_string())))
        );
        let stopped = r#"{"BackendState":"Stopped","Self":{"DNSName":""}}"#;
        assert_eq!(status_of(stopped), Ok(("Stopped".to_string(), None)));
        assert!(status_of("not json").is_err());
    }

    #[test]
    fn serving_means_the_port_is_passed_on_to_the_app() {
        let served = r#"{
            "TCP": {"8766": {"HTTPS": true}},
            "Web": {"pc.tail1234.ts.net:8766": {"Handlers": {"/": {"Proxy": "http://127.0.0.1:8766"}}}}
        }"#;
        assert!(serves(served, "pc.tail1234.ts.net", 8766));
        assert!(!serves(served, "other.tail1234.ts.net", 8766));
        assert!(!serves(served, "pc.tail1234.ts.net", 8765));
        let elsewhere =
            r#"{"Web": {"pc.tail1234.ts.net:8766": {"Handlers": {"/": {"Proxy": "http://127.0.0.1:3000"}}}}}"#;
        assert!(!serves(elsewhere, "pc.tail1234.ts.net", 8766));
        assert!(!serves("{}", "pc.tail1234.ts.net", 8766));
    }

    #[test]
    fn the_serve_command_is_the_one_serve_runs() {
        assert_eq!(
            serve_command(8766),
            "tailscale serve --bg --https=8766 http://127.0.0.1:8766"
        );
    }
}
