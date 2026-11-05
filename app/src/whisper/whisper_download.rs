//! whisper.cpp's Windows build for this computer, from its GitHub releases, unzipped into the
//! whisper folder; and the model, from Hugging Face. Both go through llama's download code, so
//! they pause, resume and stop in the task manager the same way.

use std::path::Path;

use super::{Build, MODEL_FILE, MODEL_URL, Whisper, whisper_cli};
use crate::llama::download::{self, Asset, Downloads};

/// whisper.cpp's releases, newest first, as GitHub's API lists them. Its builds come with the
/// bNNNN releases, not the vX.Y.Z ones, so several are looked through.
pub const RELEASES: &str = "https://api.github.com/repos/ggml-org/whisper.cpp/releases";

/// The folder inside the whisper folder that the zip is unzipped into before it is all there.
#[cfg(windows)]
const STAGING_FOLDER: &str = ".whisper-cpp-download";

/// The Windows CPU build's zip.
const CPU_ZIP: &str = "whisper-bin-x64.zip";

/// The start of the Windows CUDA builds' zips (whisper-bin-win-cuda-12.4.0-x64.zip); CUDA 12,
/// which current NVIDIA drivers run, rather than 11.
const CUDA_ZIP_START: &str = "whisper-bin-win-cuda-12.";

/// The end of every 64-bit Windows zip.
const X64_ZIP_END: &str = "-x64.zip";

/// The zip to download for build, the newest CUDA 12 version when there are several. Err when
/// the release has none.
pub fn zip_for(build: Build, assets: &[Asset]) -> Result<Vec<Asset>, String> {
    let is_build = |asset: &&Asset| match build {
        Build::Cpu => asset.name == CPU_ZIP,
        Build::Cuda => asset.name.starts_with(CUDA_ZIP_START) && asset.name.ends_with(X64_ZIP_END),
    };
    let mut builds: Vec<&Asset> = assets.iter().filter(is_build).collect();
    builds.sort_by(|a, b| a.name.cmp(&b.name));
    builds
        .last()
        .map(|chosen| vec![(*chosen).clone()])
        .ok_or_else(|| "There's no whisper.cpp build for Windows in the release.".to_string())
}

/// What the task manager and the messages call build's download.
pub fn program_label(build: Build) -> &'static str {
    match build {
        Build::Cuda => "whisper.cpp (CUDA)",
        Build::Cpu => "whisper.cpp",
    }
}

/// Downloads whisper.cpp's build for this computer into the whisper folder and unzips it there.
#[cfg(windows)]
pub async fn download_program(whisper: &Whisper) -> Result<(), String> {
    let folder = whisper.folder();
    let downloads = &whisper.downloads;
    let client = download::client()?;
    let build = whisper.build();
    let (release, zips) =
        download::newest_release_having(&client, RELEASES, program_label(build), |assets| zip_for(build, assets))
            .await?;
    // Unzipped into the staging folder first and moved in only when whole, so a half-finished
    // download never looks like an installed whisper-cli.
    let staging = folder.join(STAGING_FOLDER);
    if staging.exists() {
        std::fs::remove_dir_all(&staging).map_err(|e| format!("Couldn't clear {}: {e}", staging.display()))?;
    }
    std::fs::create_dir_all(&staging).map_err(|e| format!("Couldn't make {}: {e}", staging.display()))?;
    for zip in &zips {
        let zip_path = folder.join(&zip.name);
        download::download_file(&client, &zip.browser_download_url, &zip_path, downloads).await?;
        downloads.unzip(&zip_path, &staging).await?;
    }
    flatten_release_folder(&staging)?;
    if !staging.join(whisper_cli::PROGRAM_FILE).is_file() {
        return Err(format!(
            "whisper.cpp {} was unzipped, but there's no {} in it.",
            release.tag_name,
            whisper_cli::PROGRAM_FILE
        ));
    }
    download::move_into(&staging, folder)
        .map_err(|e| format!("Couldn't move whisper.cpp into {}: {e}", folder.display()))?;
    println!("whisper: whisper.cpp {} is in {}.", release.tag_name, folder.display());
    Ok(())
}

/// Elsewhere whisper.cpp ships no build to download.
#[cfg(not(windows))]
pub async fn download_program(_whisper: &Whisper) -> Result<(), String> {
    Err(format!(
        "whisper.cpp has no ready-made build for this system. Build it or install it so {} is on PATH.",
        whisper_cli::PROGRAM_FILE
    ))
}

/// whisper.cpp's zips keep their programs in a Release folder: its files are moved up into
/// staging itself, so whisper-cli sits at the whisper folder's top like llama-server does.
#[cfg_attr(not(windows), allow(dead_code))]
fn flatten_release_folder(staging: &Path) -> Result<(), String> {
    let release = staging.join("Release");
    if !release.is_dir() {
        return Ok(());
    }
    download::move_into(&release, staging).map_err(|e| format!("Couldn't move {} up: {e}", release.display()))
}

/// Downloads the model into the whisper folder.
pub async fn download_model(folder: &Path, downloads: &Downloads) -> Result<(), String> {
    std::fs::create_dir_all(folder).map_err(|e| format!("Couldn't make {}: {e}", folder.display()))?;
    download::download_file(&download::client()?, MODEL_URL, &folder.join(MODEL_FILE), downloads).await?;
    println!("whisper: {MODEL_FILE} is in {}.", folder.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str) -> Asset {
        Asset {
            name: name.to_string(),
            browser_download_url: format!("https://example.com/{name}"),
        }
    }

    fn release() -> Vec<Asset> {
        [
            "whisper-bin-Win32.zip",
            "whisper-bin-x64.zip",
            "whisper-blas-bin-x64.zip",
            "whisper-bin-win-cuda-11.8.0-x64.zip",
            "whisper-bin-win-cuda-12.4.0-x64.zip",
            "whisper-bin-win-cuda-13.4-arm64.zip",
            "whisper-bin-ubuntu-x64.tar.gz",
        ]
        .into_iter()
        .map(asset)
        .collect()
    }

    #[test]
    fn a_computer_without_an_nvidia_card_gets_the_cpu_zip() {
        let zips = zip_for(Build::Cpu, &release()).unwrap();
        assert_eq!(zips, vec![asset("whisper-bin-x64.zip")]);
    }

    #[test]
    fn an_nvidia_card_gets_the_cuda_12_zip_for_x64() {
        let zips = zip_for(Build::Cuda, &release()).unwrap();
        assert_eq!(zips, vec![asset("whisper-bin-win-cuda-12.4.0-x64.zip")]);
    }

    #[test]
    fn a_release_without_builds_is_skipped() {
        assert!(zip_for(Build::Cpu, &[asset("v1.9.5.zip")]).is_err());
    }
}
