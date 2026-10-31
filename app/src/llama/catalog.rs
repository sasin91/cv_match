//! The models the setup wizard offers to download: Qwen's own GGUF builds of Qwen3, from small to
//! large, as penny offers qwen3:4b and qwen3:8b by the card's memory. Each is weighed with
//! model_choice before it is downloaded, by the size Hugging Face lists for it.

use std::path::Path;

use super::gguf::GgufFile;

/// Bytes in a decimal gigabyte, which Hugging Face lists file sizes in.
const GB: u64 = 1_000_000_000;

/// A model to download.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatalogModel {
    /// The Hugging Face repository.
    pub repo: &'static str,
    /// The file in it, which is also its name in the models folder.
    pub file: &'static str,
    /// What the wizard calls it.
    pub label: &'static str,
    /// Its size as Hugging Face lists it, in bytes.
    pub bytes: u64,
    /// Its transformer layers (Qwen3's config.json num_hidden_layers).
    pub layers: u32,
}

impl CatalogModel {
    /// Where it downloads from.
    pub fn url(&self) -> String {
        format!("https://huggingface.co/{}/resolve/main/{}", self.repo, self.file)
    }

    /// As the weighing sees it before it is downloaded.
    pub fn as_gguf(&self, models_folder: &Path) -> GgufFile {
        GgufFile {
            name: self.file.to_string(),
            path: models_folder.join(self.file),
            bytes: self.bytes,
            layers: Some(self.layers),
        }
    }
}

/// The models offered, smallest first.
pub const MODELS: [CatalogModel; 4] = [
    CatalogModel {
        repo: "Qwen/Qwen3-1.7B-GGUF",
        file: "Qwen3-1.7B-Q8_0.gguf",
        label: "Qwen3 1.7B: small and quick, for a computer without a graphics card",
        // 1.83 GB.
        bytes: 183 * GB / 100,
        layers: 28,
    },
    CatalogModel {
        repo: "Qwen/Qwen3-4B-GGUF",
        file: "Qwen3-4B-Q4_K_M.gguf",
        label: "Qwen3 4B: penny's pick for a card with 6 GB",
        // 2.5 GB.
        bytes: 25 * GB / 10,
        layers: 36,
    },
    CatalogModel {
        repo: "Qwen/Qwen3-8B-GGUF",
        file: "Qwen3-8B-Q4_K_M.gguf",
        label: "Qwen3 8B: penny's pick for a card with 12 GB, and the best that fits 8 GB",
        // 5.03 GB.
        bytes: 503 * GB / 100,
        layers: 36,
    },
    CatalogModel {
        repo: "Qwen/Qwen3-14B-GGUF",
        file: "Qwen3-14B-Q4_K_M.gguf",
        label: "Qwen3 14B: better answers, for a card with 12 GB or more",
        // 9.0 GB.
        bytes: 90 * GB / 10,
        layers: 40,
    },
];

/// The catalog model with this file name.
pub fn find(file: &str) -> Option<&'static CatalogModel> {
    MODELS.iter().find(|model| model.file == file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_downloads_from_its_repository() {
        assert_eq!(
            find("Qwen3-8B-Q4_K_M.gguf").unwrap().url(),
            "https://huggingface.co/Qwen/Qwen3-8B-GGUF/resolve/main/Qwen3-8B-Q4_K_M.gguf"
        );
        assert_eq!(find("nonsense.gguf"), None);
    }
}
