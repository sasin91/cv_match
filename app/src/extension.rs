//! The Jobnet extension (extensions/jobnet-joblog), shipped inside the app. Browsers load an
//! extension of one's own from a folder ("Load unpacked"), so the app writes its copy out next
//! to the database at every start, and the browser picks up a newer one when it is reloaded.

use std::path::{Path, PathBuf};

use rust_embed::RustEmbed;

/// The extension's ID, fixed by the public key in its manifest ("key"), so the server can let
/// it, and only it, send changes (server.rs). It is the first 32 hex digits of the key's
/// SHA-256, written with the letters a to p.
pub const ID: &str = "jijhihfppobcoconjjllmffbohpffbll";

/// The folder the extension is written to, next to the database.
pub const FOLDER: &str = "jobnet-joblog";

#[derive(RustEmbed)]
#[folder = "../../extensions/jobnet-joblog/"]
#[include = "manifest.json"]
#[include = "*.js"]
#[include = "icons/*"]
struct Files;

/// Writes the extension into `folder`, replacing what an older version left, and answers the
/// folder.
pub fn write(folder: &Path) -> std::io::Result<PathBuf> {
    for name in Files::iter() {
        let Some(file) = Files::get(&name) else {
            continue;
        };
        let path = folder.join(name.as_ref());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, file.data)?;
    }
    // The browser asks for the folder by its full path.
    Ok(std::path::absolute(folder).unwrap_or_else(|_| folder.to_path_buf()))
}

/// The extension's version, from its manifest.
pub fn version() -> String {
    Files::get("manifest.json")
        .and_then(|file| serde_json::from_slice::<serde_json::Value>(&file.data).ok())
        .and_then(|manifest| manifest["version"].as_str().map(str::to_string))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_id_is_the_manifest_keys() {
        use base64::Engine;
        use sha2::Digest;
        let manifest: serde_json::Value = serde_json::from_slice(&Files::get("manifest.json").unwrap().data).unwrap();
        let key = base64::engine::general_purpose::STANDARD
            .decode(manifest["key"].as_str().unwrap())
            .unwrap();
        let digest = sha2::Sha256::digest(&key);
        // Each hex digit of the first 16 bytes as a letter, 0 as a.
        let id: String = digest[..16]
            .iter()
            .flat_map(|byte| [byte >> 4, byte & 0x0f])
            .map(|digit| (b'a' + digit) as char)
            .collect();
        assert_eq!(id, ID);
    }

    #[test]
    fn the_extension_is_written_out_whole() {
        let folder = std::env::temp_dir().join(format!("recruiter-extension-{}", std::process::id()));
        write(&folder).unwrap();
        for name in ["manifest.json", "background.js", "content.js", "icons/icon-128.png"] {
            assert!(folder.join(name).is_file(), "{name}");
        }
        assert!(!folder.join("store").exists());
        std::fs::remove_dir_all(&folder).unwrap();
        assert!(!version().is_empty());
    }
}
