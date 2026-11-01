//! llama-server, run by the app for the "llama_cpp" provider: started on the first call with the
//! model model_choice picks, on a free port on 127.0.0.1, and kept running for the next calls.
//! Its OpenAI-compatible API is called through the openai adapter as any local server's is.
//!
//! It stops with the app: on Windows it runs in a job object that closes, and so ends it, when
//! the app ends, however the app ends (closing its window included). Its output goes to
//! llama-server.log in the llama folder, whose last lines are shown when it fails to start.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use super::download::Downloads;
use super::gguf::{self, GgufFile};
use super::hardware::Inventory;
use super::model_choice::{self, Fit};

/// The llama.cpp program in the llama folder.
#[cfg(windows)]
pub const SERVER_FILE: &str = "llama-server.exe";
#[cfg(not(windows))]
pub const SERVER_FILE: &str = "llama-server";

/// The folder of GGUF models inside the llama folder.
pub const MODELS_FOLDER: &str = "models";

/// llama-server's output, in the llama folder.
pub const LOG_FILE: &str = "llama-server.log";

/// The context the server keeps, in tokens: a job post, a CV and a tailored résumé's answer.
pub const CONTEXT_TOKENS: u32 = 16_384;

/// How long a model may take to load before its start counts as failed, in seconds: a 20 GB file
/// from a hard disk.
const LOAD_SECONDS: u64 = 300;

/// How often the health check asks while the model loads, in milliseconds.
const HEALTH_POLL_MILLISECONDS: u64 = 250;

/// How long one health check may take, in seconds.
const HEALTH_CHECK_SECONDS: u64 = 2;

/// The log lines shown when the server fails to start.
const LOG_TAIL_LINES: usize = 12;

/// program's full path on PATH, with links followed, when it is there.
fn on_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|folder| folder.join(program))
        .find(|candidate| candidate.is_file())
        .map(|found| std::fs::canonicalize(&found).unwrap_or(found))
}

/// Where the running server answers, and the model it answers as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub base_url: String,
    pub model: String,
}

/// The server process and what it runs.
struct Running {
    child: Child,
    /// Ends the child with the app; dropped (closed) after the child is told to stop.
    _job: job::KillOnClose,
    model: PathBuf,
    endpoint: Endpoint,
}

/// What the API key page shows of the llama folder: the program, the computer, and each model
/// with how it fits.
pub struct Overview {
    pub folder: PathBuf,
    /// The llama-server that runs, when there is one.
    pub server: Option<PathBuf>,
    pub inventory: Inventory,
    /// Why the graphics card can't be used, when it can't.
    pub card_problem: Option<String>,
    pub models: Vec<(GgufFile, Fit)>,
    /// The model a blank model field runs, or why none can.
    pub chosen: Result<String, String>,
}

/// The llama folder (llama-server and its libraries, and models\*.gguf), and the server once
/// started.
pub struct LlamaServer {
    folder: PathBuf,
    inventory: Inventory,
    running: Mutex<Option<Running>>,
    /// The setup wizard's download.
    pub downloads: Downloads,
}

impl LlamaServer {
    /// The server for folder, made absolute: llama-server runs inside the folder, where a relative
    /// path to itself or its model would point somewhere else.
    pub fn new(folder: &Path, inventory: Inventory) -> Self {
        LlamaServer {
            folder: std::path::absolute(folder).unwrap_or_else(|_| folder.to_path_buf()),
            inventory,
            running: Mutex::new(None),
            downloads: Downloads::default(),
        }
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    pub fn models_folder(&self) -> PathBuf {
        self.folder.join(MODELS_FOLDER)
    }

    pub fn inventory(&self) -> &Inventory {
        &self.inventory
    }

    /// The llama-server to run: the llama folder's, else one already on this computer's PATH
    /// (winget's llama.cpp, say), followed through its link to the folder its DLLs are in.
    pub fn server_path(&self) -> Option<PathBuf> {
        let own = self.folder.join(SERVER_FILE);
        if own.is_file() {
            return Some(own);
        }
        on_path(SERVER_FILE)
    }

    /// The models in the models folder.
    pub fn models(&self) -> Vec<GgufFile> {
        gguf::list(&self.models_folder())
    }

    /// The folder as the API key page shows it.
    /// Whether the card can run a model, as model_choice::card_usable says for the llama-server
    /// here. Before there is one, the card counts as usable: the setup wizard downloads the build
    /// for it, so there is nothing missing to say yet.
    pub fn card_usable(&self) -> Result<bool, String> {
        let Some(server) = self.server_path() else {
            return Ok(self.inventory.gpu.is_some());
        };
        let server_folder = server.parent().unwrap_or(&self.folder);
        model_choice::card_usable(&self.inventory, &model_choice::compute_libraries(server_folder))
    }

    pub fn overview(&self) -> Overview {
        let card = self.card_usable();
        let card_usable = card.as_ref().is_ok_and(|usable| *usable);
        let models = self.models();
        let chosen =
            model_choice::choose(&models, "", &self.inventory, card_usable).map(|(model, _)| model.name.clone());
        Overview {
            folder: self.folder.clone(),
            server: self.server_path(),
            inventory: self.inventory.clone(),
            card_problem: card.err(),
            models: models
                .iter()
                .map(|model| (model.clone(), model_choice::fit(model, &self.inventory, card_usable)))
                .collect(),
            chosen,
        }
    }

    /// The server running named (a file in the models folder, or "" for the one that fits best),
    /// started or restarted with it when needed. Err says what's missing or why it didn't start.
    pub async fn start(&self, named: &str) -> Result<Endpoint, String> {
        let Some(server) = self.server_path() else {
            return Err(format!(
                "There's no {SERVER_FILE} in {}. Download it on the API key page's \"Set it up\" (/llama_setup).",
                self.folder.display()
            ));
        };
        let card = self.card_usable()?;
        let models = self.models();
        let (model, fit) = model_choice::choose(&models, named, &self.inventory, card)?;
        let mut running = self.running.lock().await;
        if let Some(current) = running.as_mut()
            && current.model == model.path
            && matches!(current.child.try_wait(), Ok(None))
        {
            return Ok(current.endpoint.clone());
        }
        if let Some(mut previous) = running.take() {
            stop(&mut previous).await;
        }
        let started = self.spawn(&server, model, fit).await?;
        let endpoint = started.endpoint.clone();
        *running = Some(started);
        Ok(endpoint)
    }

    async fn spawn(&self, server: &Path, model: &GgufFile, fit: Fit) -> Result<Running, String> {
        let port = free_port().map_err(|e| format!("No free port for llama-server: {e}"))?;
        let log_path = self.folder.join(LOG_FILE);
        let log = File::create(&log_path).map_err(|e| format!("Couldn't write {}: {e}", log_path.display()))?;
        let log_too = log
            .try_clone()
            .map_err(|e| format!("Couldn't write {}: {e}", log_path.display()))?;
        let alias = model_alias(model);
        let mut child = Command::new(server)
            .current_dir(&self.folder)
            .args(server_arguments(model, &alias, port, fit))
            .stdin(Stdio::null())
            .stdout(log)
            .stderr(log_too)
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("{} didn't start: {e}", server.display()))?;
        let job = job::KillOnClose::assign(&child).map_err(|e| {
            crate::log_line!("llama: llama-server won't stop with the app: {e}");
            format!("llama-server couldn't be tied to the app: {e}")
        })?;
        let base = format!("http://127.0.0.1:{port}");
        println!(
            "llama: starting {} with {} ({}), log in {}",
            SERVER_FILE,
            model.name,
            fit.label(),
            log_path.display()
        );
        let started = Instant::now();
        if let Err(why) = wait_until_healthy(&base, &mut child).await {
            let _ = child.start_kill();
            return Err(format!("{why}\n{}", log_tail(&log_path)));
        }
        println!(
            "llama: {} is loaded ({:.1} s).",
            model.name,
            started.elapsed().as_secs_f64()
        );
        Ok(Running {
            child,
            _job: job,
            model: model.path.clone(),
            endpoint: Endpoint {
                base_url: format!("{base}/v1"),
                model: alias,
            },
        })
    }
}

/// The name the server answers as: the file name without .gguf.
fn model_alias(model: &GgufFile) -> String {
    model
        .path
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| model.name.clone())
}

/// llama-server's command line: the model, on 127.0.0.1 only, its context, its layers on the card,
/// and the model's own chat template (--jinja).
fn server_arguments(model: &GgufFile, alias: &str, port: u16, fit: Fit) -> Vec<String> {
    vec![
        "--model".to_string(),
        model.path.display().to_string(),
        "--alias".to_string(),
        alias.to_string(),
        "--host".to_string(),
        "127.0.0.1".to_string(),
        "--port".to_string(),
        port.to_string(),
        "--ctx-size".to_string(),
        CONTEXT_TOKENS.to_string(),
        "--n-gpu-layers".to_string(),
        fit.gpu_layers().to_string(),
        "--jinja".to_string(),
    ]
}

/// A port nothing listens on now. Another program could take it before llama-server does; then
/// the start fails and says so.
fn free_port() -> std::io::Result<u16> {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .and_then(|listener| listener.local_addr())
        .map(|address| address.port())
}

/// Waits for GET /health to answer 200 (it answers 503 while the model loads), failing when the
/// server ends or LOAD_SECONDS pass.
async fn wait_until_healthy(base: &str, child: &mut Child) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(HEALTH_CHECK_SECONDS))
        .build()
        .map_err(|e| format!("Couldn't make the health check: {e}"))?;
    let deadline = Instant::now() + Duration::from_secs(LOAD_SECONDS);
    let health = format!("{base}/health");
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!("llama-server stopped while loading the model ({status})."));
        }
        let healthy = client
            .get(&health)
            .send()
            .await
            .is_ok_and(|response| response.status().is_success());
        if healthy {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!("llama-server didn't load the model in {LOAD_SECONDS} seconds."));
        }
        tokio::time::sleep(Duration::from_millis(HEALTH_POLL_MILLISECONDS)).await;
    }
}

/// Stops a server and waits for it to end.
async fn stop(running: &mut Running) {
    if let Err(e) = running.child.kill().await {
        crate::log_line!("llama: the previous llama-server couldn't be stopped: {e}");
    }
}

/// The log's last LOG_TAIL_LINES lines, or why it can't be read.
fn log_tail(path: &Path) -> String {
    match std::fs::read(path) {
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes);
            let lines: Vec<&str> = text.lines().collect();
            let tail = &lines[lines.len().saturating_sub(LOG_TAIL_LINES)..];
            format!("The end of {}:\n{}", path.display(), tail.join("\n"))
        }
        Err(e) => format!("{} couldn't be read: {e}", path.display()),
    }
}

#[cfg(windows)]
mod job {
    use tokio::process::Child;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, SetInformationJobObject,
    };

    /// A job object that ends its processes when it is closed, by drop or by the app ending.
    pub struct KillOnClose(HANDLE);

    // SAFETY: the handle is only closed, once, in drop; a job handle may be used from any thread.
    unsafe impl Send for KillOnClose {}
    // SAFETY: as above; no method reads or changes the job through a shared reference.
    unsafe impl Sync for KillOnClose {}

    impl KillOnClose {
        /// A new job with child in it.
        pub fn assign(child: &Child) -> Result<Self, String> {
            let process = child.raw_handle().ok_or("the process has already ended")?;
            // SAFETY: no name or security attributes; the handle is owned by the returned value.
            let job = KillOnClose(unsafe { CreateJobObjectW(None, None) }.map_err(|e| e.to_string())?);
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            // SAFETY: limits is the JOBOBJECT_EXTENDED_LIMIT_INFORMATION the class names, with its size.
            unsafe {
                SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    (&raw const limits).cast(),
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            }
            .map_err(|e| e.to_string())?;
            // SAFETY: both handles are open: the job above, and the child's process, which tokio keeps.
            unsafe { AssignProcessToJobObject(job.0, HANDLE(process)) }.map_err(|e| e.to_string())?;
            Ok(job)
        }
    }

    impl Drop for KillOnClose {
        fn drop(&mut self) {
            // SAFETY: the handle is open and closed only here.
            if let Err(e) = unsafe { CloseHandle(self.0) } {
                crate::log_line!("llama: the job object couldn't be closed: {e}");
            }
        }
    }
}

#[cfg(not(windows))]
mod job {
    use tokio::process::Child;

    /// Elsewhere the server only stops with the app through kill_on_drop.
    pub struct KillOnClose;

    impl KillOnClose {
        pub fn assign(_child: &Child) -> Result<Self, String> {
            Ok(KillOnClose)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llama::hardware::GIB;

    fn model() -> GgufFile {
        GgufFile {
            name: "Qwen3-8B-Q4_K_M.gguf".to_string(),
            path: PathBuf::from("models").join("Qwen3-8B-Q4_K_M.gguf"),
            bytes: 5 * GIB,
            layers: Some(36),
        }
    }

    #[test]
    fn the_server_listens_on_this_computer_only_with_the_chosen_layers_on_the_card() {
        let arguments = server_arguments(&model(), "Qwen3-8B-Q4_K_M", 50123, Fit::Split { gpu_layers: 20 });
        let pairs: Vec<(&str, &str)> = arguments
            .chunks(2)
            .filter(|pair| pair.len() == 2)
            .map(|pair| (pair[0].as_str(), pair[1].as_str()))
            .collect();
        assert!(pairs.contains(&("--host", "127.0.0.1")));
        assert!(pairs.contains(&("--port", "50123")));
        assert!(pairs.contains(&("--n-gpu-layers", "20")));
        assert!(pairs.contains(&("--alias", "Qwen3-8B-Q4_K_M")));
        assert_eq!(arguments.last().map(String::as_str), Some("--jinja"));
        assert_eq!(model_alias(&model()), "Qwen3-8B-Q4_K_M");
    }

    #[tokio::test]
    async fn without_the_program_the_folder_is_named() {
        let folder = std::env::temp_dir().join("recruiter-llama-test-no-server");
        let server = LlamaServer::new(&folder, Inventory::default());
        let why = server.start("").await.unwrap_err();
        assert!(why.starts_with(&format!("There's no {SERVER_FILE} in ")), "{why}");
        assert!(server.folder().is_absolute());
        let relative = LlamaServer::new(Path::new("llama"), Inventory::default());
        assert!(relative.folder().is_absolute() && relative.folder().ends_with("llama"));
        let overview = server.overview();
        assert_eq!(overview.server.is_some(), on_path(SERVER_FILE).is_some());
        assert_eq!(
            overview.chosen,
            Err("There's no .gguf model in the models folder yet.".to_string())
        );
    }

    #[test]
    fn an_amd_card_is_not_said_to_lack_vulkan_before_llama_server_is_there() {
        let folder = std::env::temp_dir().join("recruiter-llama-test-amd-no-server");
        let amd = Inventory {
            memory_bytes: 32 * GIB,
            gpu: Some(crate::llama::hardware::Gpu {
                name: "AMD Radeon RX 7600".to_string(),
                vendor: crate::llama::hardware::GpuVendor::Amd,
                vram_bytes: 8 * GIB,
            }),
        };
        let server = LlamaServer::new(&folder, amd);
        assert_eq!(server.card_usable(), Ok(true));
        assert_eq!(server.overview().card_problem, None);
    }
}
