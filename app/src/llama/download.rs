//! The setup wizard's downloads: llama.cpp's Windows build for this computer's graphics card,
//! from its latest GitHub release, unzipped into the llama folder; and a catalog model into the
//! models folder. One download runs at a time, in the background, and Downloads says how far it
//! is for the page to show.
//!
//! A file is written as name.part and renamed when it is whole, so a broken download never looks
//! like a model or a server.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::Deserialize;
use tokio::io::AsyncWriteExt;

use super::catalog::CatalogModel;
use super::hardware::{GpuVendor, Inventory};
use super::llama_server::SERVER_FILE;

/// llama.cpp's latest release, as GitHub's API describes it.
pub const LATEST_RELEASE: &str = "https://api.github.com/repos/ggml-org/llama.cpp/releases/latest";

/// What the app calls itself to GitHub, whose API refuses requests without a User-Agent.
const USER_AGENT: &str = "recruiter-desktop";

/// How long connecting may take, in seconds.
const CONNECT_SECONDS: u64 = 15;

/// How long the download may stall between two pieces before it counts as failed, in seconds.
const STALL_SECONDS: u64 = 60;

/// The suffix of a file being downloaded.
const PART_SUFFIX: &str = ".part";

/// A release file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
}

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

/// A llama.cpp build for Windows, by the name part its zip carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Build {
    /// NVIDIA cards, with CUDA's runtime in a second zip.
    Cuda,
    /// AMD, Intel and other cards, through the graphics driver alone.
    Vulkan,
    /// No graphics card.
    Cpu,
}

impl Build {
    /// The build for this computer's card. An AMD card gets Vulkan rather than HIP: HIP needs
    /// AMD's HIP SDK installed, Vulkan only the graphics driver.
    pub fn for_computer(inventory: &Inventory) -> Self {
        match inventory.gpu.as_ref().map(|gpu| gpu.vendor) {
            Some(GpuVendor::Nvidia) => Build::Cuda,
            Some(_) => Build::Vulkan,
            None => Build::Cpu,
        }
    }

    /// What the wizard calls it.
    pub fn label(self) -> &'static str {
        match self {
            Build::Cuda => "the CUDA build, for NVIDIA cards",
            Build::Vulkan => "the Vulkan build, which runs on the graphics driver",
            Build::Cpu => "the CPU build, as there is no graphics card",
        }
    }

    /// The text in the release zip's name, after "-bin-".
    fn marker(self) -> &'static str {
        match self {
            Build::Cuda => "-bin-win-cuda-",
            Build::Vulkan => "-bin-win-vulkan-x64.zip",
            Build::Cpu => "-bin-win-cpu-x64.zip",
        }
    }
}

/// The zips to download for build: llama.cpp's own, and for CUDA the matching CUDA runtime
/// (cudart-llama-bin-win-cuda-12.4-x64.zip beside llama-b1234-bin-win-cuda-12.4-x64.zip), taking
/// the newest CUDA version when there are several. Err when the release has none.
pub fn assets_for(build: Build, assets: &[Asset]) -> Result<Vec<Asset>, String> {
    let is_build = |asset: &&Asset| {
        asset.name.starts_with("llama-") && asset.name.contains(build.marker()) && asset.name.ends_with("-x64.zip")
    };
    let mut builds: Vec<&Asset> = assets.iter().filter(is_build).collect();
    builds.sort_by(|a, b| a.name.cmp(&b.name));
    let Some(chosen) = builds.last() else {
        return Err(format!(
            "llama.cpp's latest release has no {} for Windows.",
            build.label()
        ));
    };
    let mut wanted = vec![(*chosen).clone()];
    if build == Build::Cuda {
        let version = chosen.name.split(build.marker()).nth(1).unwrap_or_default();
        let runtime_name = format!("cudart-llama-bin-win-cuda-{version}");
        let runtime = assets
            .iter()
            .find(|asset| asset.name == runtime_name)
            .ok_or_else(|| format!("llama.cpp's latest release has no {runtime_name}."))?;
        wanted.push(runtime.clone());
    }
    Ok(wanted)
}

/// How the download is going.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Progress {
    #[default]
    Idle,
    Running {
        what: String,
        done_bytes: u64,
        total_bytes: Option<u64>,
    },
    Finished {
        what: String,
    },
    Failed {
        what: String,
        why: String,
    },
}

/// The one download, and how it is going.
#[derive(Debug, Default)]
pub struct Downloads {
    progress: Mutex<Progress>,
}

impl Downloads {
    pub fn progress(&self) -> Progress {
        self.lock().clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Progress> {
        self.progress.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Marks a download of what as started, or Err when one is already running.
    pub fn begin(&self, what: &str) -> Result<(), String> {
        let mut progress = self.lock();
        if let Progress::Running { what, .. } = &*progress {
            return Err(format!("{what} is still downloading. Wait for it to finish."));
        }
        *progress = Progress::Running {
            what: what.to_string(),
            done_bytes: 0,
            total_bytes: None,
        };
        Ok(())
    }

    fn set_bytes(&self, done_bytes: u64, total_bytes: Option<u64>) {
        if let Progress::Running {
            done_bytes: done,
            total_bytes: total,
            ..
        } = &mut *self.lock()
        {
            *done = done_bytes;
            *total = total_bytes;
        }
    }

    /// Ends the running download with its outcome; a failure is logged.
    pub fn end(&self, outcome: Result<(), String>) {
        let mut progress = self.lock();
        let what = match &*progress {
            Progress::Running { what, .. } => what.clone(),
            _ => return,
        };
        *progress = match outcome {
            Ok(()) => Progress::Finished { what },
            Err(why) => {
                eprintln!("llama: downloading {what} failed: {why}");
                Progress::Failed { what, why }
            }
        };
    }
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(CONNECT_SECONDS))
        .read_timeout(Duration::from_secs(STALL_SECONDS))
        .build()
        .map_err(|e| format!("The app couldn't make its HTTP client: {e}"))
}

/// Downloads llama.cpp's build for this computer into folder and unzips it there.
pub async fn download_server(folder: &Path, inventory: &Inventory, downloads: &Downloads) -> Result<(), String> {
    let client = client()?;
    let release: Release = client
        .get(LATEST_RELEASE)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| format!("GitHub didn't list llama.cpp's latest release: {e}"))?
        .json()
        .await
        .map_err(|e| format!("GitHub's answer about llama.cpp's release couldn't be read: {e}"))?;
    let build = Build::for_computer(inventory);
    let assets = assets_for(build, &release.assets)?;
    std::fs::create_dir_all(folder).map_err(|e| format!("Couldn't make {}: {e}", folder.display()))?;
    for asset in &assets {
        let zip_path = folder.join(&asset.name);
        download_file(&client, &asset.browser_download_url, &zip_path, downloads).await?;
        let (zip, into) = (zip_path.clone(), folder.to_path_buf());
        tokio::task::spawn_blocking(move || unzip(&zip, &into))
            .await
            .map_err(|e| format!("Unzipping {} stopped: {e}", asset.name))?
            .map_err(|e| format!("Couldn't unzip {}: {e}", asset.name))?;
        if let Err(e) = std::fs::remove_file(&zip_path) {
            eprintln!("llama: {} couldn't be deleted after unzipping: {e}", zip_path.display());
        }
    }
    if !folder.join(SERVER_FILE).is_file() {
        return Err(format!(
            "llama.cpp {} was unzipped, but there's no {SERVER_FILE} in {}.",
            release.tag_name,
            folder.display()
        ));
    }
    println!(
        "llama: llama.cpp {} ({}) is in {}.",
        release.tag_name,
        build.label(),
        folder.display()
    );
    Ok(())
}

/// Downloads model into models_folder.
pub async fn download_model(model: &CatalogModel, models_folder: &Path, downloads: &Downloads) -> Result<(), String> {
    std::fs::create_dir_all(models_folder).map_err(|e| format!("Couldn't make {}: {e}", models_folder.display()))?;
    download_file(&client()?, &model.url(), &models_folder.join(model.file), downloads).await?;
    println!("llama: {} is in {}.", model.file, models_folder.display());
    Ok(())
}

/// url's body into to, through to.part, counting bytes into downloads.
async fn download_file(client: &reqwest::Client, url: &str, to: &Path, downloads: &Downloads) -> Result<(), String> {
    let mut response = client
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| format!("{url} couldn't be downloaded: {e}"))?;
    let total = response.content_length();
    let part = part_path(to);
    let mut file = tokio::fs::File::create(&part)
        .await
        .map_err(|e| format!("Couldn't write {}: {e}", part.display()))?;
    let mut done = 0u64;
    downloads.set_bytes(done, total);
    while let Some(piece) = response
        .chunk()
        .await
        .map_err(|e| format!("The download of {url} broke off: {e}"))?
    {
        file.write_all(&piece)
            .await
            .map_err(|e| format!("Couldn't write {}: {e}", part.display()))?;
        done += piece.len() as u64;
        downloads.set_bytes(done, total);
    }
    file.flush()
        .await
        .map_err(|e| format!("Couldn't write {}: {e}", part.display()))?;
    drop(file);
    if total.is_some_and(|total| total != done) {
        return Err(format!("The download of {url} ended early ({done} bytes)."));
    }
    tokio::fs::rename(&part, to)
        .await
        .map_err(|e| format!("Couldn't move {} to {}: {e}", part.display(), to.display()))
}

fn part_path(to: &Path) -> PathBuf {
    let mut name = to.file_name().unwrap_or_default().to_os_string();
    name.push(PART_SUFFIX);
    to.with_file_name(name)
}

/// Unzips zip into folder, refusing entries that would land outside it.
fn unzip(zip: &Path, folder: &Path) -> io::Result<()> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(zip)?).map_err(io::Error::other)?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(io::Error::other)?;
        let Some(relative) = entry.enclosed_name() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{} would unzip outside the folder", entry.name().unwrap_or_default()),
            ));
        };
        let target = folder.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        io::copy(&mut entry, &mut std::fs::File::create(&target)?)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use crate::llama::hardware::{GIB, Gpu};

    fn asset(name: &str) -> Asset {
        Asset {
            name: name.to_string(),
            browser_download_url: format!("https://github.com/ggml-org/llama.cpp/releases/download/b7000/{name}"),
        }
    }

    fn release() -> Vec<Asset> {
        [
            "cudart-llama-bin-win-cuda-12.4-x64.zip",
            "cudart-llama-bin-win-cuda-13.1-x64.zip",
            "llama-b7000-bin-macos-arm64.zip",
            "llama-b7000-bin-win-cpu-arm64.zip",
            "llama-b7000-bin-win-cpu-x64.zip",
            "llama-b7000-bin-win-cuda-12.4-x64.zip",
            "llama-b7000-bin-win-cuda-13.1-x64.zip",
            "llama-b7000-bin-win-hip-radeon-x64.zip",
            "llama-b7000-bin-win-vulkan-x64.zip",
        ]
        .into_iter()
        .map(asset)
        .collect()
    }

    fn names(assets: &[Asset]) -> Vec<&str> {
        assets.iter().map(|asset| asset.name.as_str()).collect()
    }

    #[test]
    fn each_card_gets_its_build() {
        let card = |vendor| Inventory {
            memory_bytes: 32 * GIB,
            gpu: Some(Gpu {
                name: "a card".to_string(),
                vendor,
                vram_bytes: 8 * GIB,
            }),
        };
        assert_eq!(Build::for_computer(&card(GpuVendor::Amd)), Build::Vulkan);
        assert_eq!(Build::for_computer(&card(GpuVendor::Nvidia)), Build::Cuda);
        assert_eq!(Build::for_computer(&Inventory::default()), Build::Cpu);
        assert_eq!(
            names(&assets_for(Build::Vulkan, &release()).unwrap()),
            ["llama-b7000-bin-win-vulkan-x64.zip"]
        );
        assert_eq!(
            names(&assets_for(Build::Cpu, &release()).unwrap()),
            ["llama-b7000-bin-win-cpu-x64.zip"]
        );
        assert_eq!(
            names(&assets_for(Build::Cuda, &release()).unwrap()),
            [
                "llama-b7000-bin-win-cuda-13.1-x64.zip",
                "cudart-llama-bin-win-cuda-13.1-x64.zip"
            ]
        );
        assert!(assets_for(Build::Vulkan, &[]).unwrap_err().contains("Vulkan build"));
    }

    #[test]
    fn one_download_runs_at_a_time() {
        let downloads = Downloads::default();
        downloads.begin("llama.cpp").unwrap();
        assert!(downloads.begin("Qwen3-8B-Q4_K_M.gguf").is_err());
        downloads.end(Err("offline".to_string()));
        assert_eq!(
            downloads.progress(),
            Progress::Failed {
                what: "llama.cpp".to_string(),
                why: "offline".to_string()
            }
        );
        downloads.begin("Qwen3-8B-Q4_K_M.gguf").unwrap();
    }

    #[test]
    fn a_zip_unzips_into_the_folder_only() {
        let folder = std::env::temp_dir().join(format!("recruiter-unzip-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let zip_path = folder.join("build.zip");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        writer.start_file("llama-server.exe", options).unwrap();
        writer.write_all(b"MZ").unwrap();
        writer.finish().unwrap();
        unzip(&zip_path, &folder).unwrap();
        assert_eq!(std::fs::read(folder.join("llama-server.exe")).unwrap(), b"MZ");

        let evil_path = folder.join("evil.zip");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&evil_path).unwrap());
        writer.start_file("../outside.txt", options).unwrap();
        writer.finish().unwrap();
        assert!(unzip(&evil_path, &folder).is_err());
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
