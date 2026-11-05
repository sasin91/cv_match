//! whisper: speech to text on this computer, with whisper.cpp's whisper-cli, for the recordings
//! (recordings): a call, a voice message or a spoken note. Nothing leaves the computer.
//!
//! Its folder is `whisper` next to the database; it holds whisper-cli with its DLLs, unzipped
//! from a whisper.cpp release (whisper_download), and the model file. Both are downloaded the
//! first time something is transcribed, each as a Download row in the task manager. Elsewhere than
//! Windows whisper.cpp ships no build for the app to download, so whisper-cli is used from PATH.

pub mod whisper_cli;
pub mod whisper_download;

use std::path::{Path, PathBuf};

use crate::llama::download::Downloads;
use crate::llama::hardware::{GpuVendor, Inventory};

/// The whisper folder's name next to the database.
pub const FOLDER: &str = "whisper";

/// The model every recording is transcribed with: Whisper large-v3-turbo, 5-bit, in whisper.cpp's
/// format (about 0.55 GB). It knows Danish and English, and reads a minute of speech in seconds on
/// a graphics card.
pub const MODEL_FILE: &str = "ggml-large-v3-turbo-q5_0.bin";

/// Where MODEL_FILE is downloaded from: whisper.cpp's own models on Hugging Face.
pub const MODEL_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin";

/// The model's name as a transcript says who wrote it (recordings.transcribed_by).
pub const MODEL_NAME: &str = "whisper large-v3-turbo";

/// The whisper.cpp build for this computer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Build {
    /// NVIDIA cards.
    Cuda,
    /// Everything else: whisper.cpp ships no Vulkan build for Windows.
    Cpu,
}

impl Build {
    pub fn for_computer(inventory: &Inventory) -> Self {
        match inventory.gpu.as_ref().map(|gpu| gpu.vendor) {
            Some(GpuVendor::Nvidia) => Build::Cuda,
            _ => Build::Cpu,
        }
    }
}

/// The whisper folder, its downloads, and the turn transcriptions take: one at a time, since one
/// already keeps the graphics card or the processor busy.
#[derive(Debug)]
pub struct Whisper {
    folder: PathBuf,
    build: Build,
    pub downloads: Downloads,
    /// Held while whisper-cli or the model is being fetched, so two recordings don't both fetch.
    pub fetching: tokio::sync::Mutex<()>,
    /// Held while a recording is transcribed.
    pub transcribing: tokio::sync::Mutex<()>,
}

impl Whisper {
    pub fn new(folder: &Path, inventory: &Inventory) -> Self {
        Whisper {
            folder: folder.to_path_buf(),
            build: Build::for_computer(inventory),
            downloads: Downloads::default(),
            fetching: tokio::sync::Mutex::new(()),
            transcribing: tokio::sync::Mutex::new(()),
        }
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    pub fn build(&self) -> Build {
        self.build
    }

    /// The model file, downloaded or not.
    pub fn model_path(&self) -> PathBuf {
        self.folder.join(MODEL_FILE)
    }

    /// whisper-cli in the whisper folder, else on PATH; None when neither has it.
    pub fn program(&self) -> Option<PathBuf> {
        let in_folder = self.folder.join(whisper_cli::PROGRAM_FILE);
        if in_folder.is_file() {
            return Some(in_folder);
        }
        on_path(whisper_cli::PROGRAM_FILE)
    }
}

/// program's full path on PATH, when it is there.
fn on_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|folder| folder.join(program))
        .find(|candidate| candidate.is_file())
}
