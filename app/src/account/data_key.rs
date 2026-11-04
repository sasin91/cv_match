//! The data key that seals the secrets table's values (key_vault). Each database has its own,
//! kept in a file beside it (`<database>-key`), never in the database, so a copy of the database
//! (a backup, a file sent with a bug report) opens none of its secrets.
//!
//! The file is protected by the system: on Windows it holds the key wrapped with DPAPI, so it only
//! opens for the Windows user who made it; elsewhere it holds the key itself, readable only by its
//! owner (mode 0600). The values themselves are sealed here, the same way on every system, with
//! XChaCha20-Poly1305: a random nonce for each, and the secret's name bound in as associated data.

use std::io::Write;
use std::path::{Path, PathBuf};

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use zeroize::Zeroizing;

/// The data key's length in bytes: XChaCha20-Poly1305's 256-bit key.
const KEY_BYTES: usize = 32;

/// A sealed value's nonce length in bytes: XChaCha20's 192-bit nonce, long enough to be random.
const NONCE_BYTES: usize = 24;

/// The first byte of a value sealed as this file seals it, so a later way can tell its own apart.
const SEALED_V1: u8 = 1;

/// What follows the database's file name in its data key file's name.
pub const KEY_FILE_SUFFIX: &str = "-key";

/// The key that seals and opens one database's secrets.
pub struct DataKey {
    cipher: XChaCha20Poly1305,
}

/// The file database's data key is kept in.
pub fn key_file(database: &Path) -> PathBuf {
    let mut name = database.as_os_str().to_owned();
    name.push(KEY_FILE_SUFFIX);
    PathBuf::from(name)
}

impl DataKey {
    /// A new random key, kept nowhere: for a database that lives in memory (tests).
    pub fn random() -> Result<DataKey, String> {
        Ok(DataKey::from_bytes(&*random_key()?))
    }

    /// database's key, read from its key file, or made and kept there when there is none yet.
    pub fn of_database(database: &Path) -> Result<DataKey, String> {
        let file = key_file(database);
        match std::fs::read(&file) {
            Ok(kept) => Ok(DataKey::from_bytes(&*key_in(&file, &kept)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => DataKey::made_in(&file),
            Err(e) => Err(format!("The key file {} couldn't be read: {e}", file.display())),
        }
    }

    /// A new key, kept in file. Two programs starting at once (the window and the scheduled
    /// scan) may both make one; the first to link its file in place wins, and the other reads it.
    fn made_in(file: &Path) -> Result<DataKey, String> {
        let key = random_key()?;
        let staging = staging_file(file);
        let written = write_private(&staging, &platform::protect(key.as_slice())?);
        let linked = written.and_then(|()| std::fs::hard_link(&staging, file));
        let _ = std::fs::remove_file(&staging);
        match linked {
            Ok(()) => {
                crate::log_line!("secrets: made the key file {}.", file.display());
                Ok(DataKey::from_bytes(&key))
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let kept = std::fs::read(file)
                    .map_err(|e| format!("The key file {} couldn't be read: {e}", file.display()))?;
                Ok(DataKey::from_bytes(&*key_in(file, &kept)?))
            }
            // A file system without hard links (FAT, some network shares): written in place,
            // still only if there is none.
            Err(_) => match write_new_private(file, &platform::protect(key.as_slice())?) {
                Ok(()) => Ok(DataKey::from_bytes(&key)),
                Err(e) => Err(format!("The key file {} couldn't be made: {e}", file.display())),
            },
        }
    }

    fn from_bytes(key: &[u8; KEY_BYTES]) -> DataKey {
        DataKey {
            cipher: XChaCha20Poly1305::new(key.into()),
        }
    }

    /// value sealed for the secret called name: SEALED_V1, the nonce, then the ciphertext.
    pub fn seal(&self, name: &str, value: &str) -> Result<Vec<u8>, String> {
        let mut nonce = [0_u8; NONCE_BYTES];
        getrandom::fill(&mut nonce).map_err(|e| format!("The system gave no random bytes to seal with: {e}"))?;
        let payload = Payload {
            msg: value.as_bytes(),
            aad: name.as_bytes(),
        };
        let ciphertext = self
            .cipher
            .encrypt(XNonce::from_slice(&nonce), payload)
            .map_err(|_| format!("The secret {name} couldn't be sealed."))?;
        let mut sealed = Vec::with_capacity(1 + NONCE_BYTES + ciphertext.len());
        sealed.push(SEALED_V1);
        sealed.extend_from_slice(&nonce);
        sealed.extend_from_slice(&ciphertext);
        Ok(sealed)
    }

    /// The value sealed for the secret called name; Err when this key didn't seal it for that
    /// name (another database's key file, or a damaged value).
    pub fn open(&self, name: &str, sealed: &[u8]) -> Result<String, String> {
        let not_opened = || {
            format!(
                "The secret {name} couldn't be opened: it was sealed with another key file than this database's (copied from another computer or user?). Save it again."
            )
        };
        let Some((&SEALED_V1, rest)) = sealed.split_first() else {
            return Err(not_opened());
        };
        if rest.len() < NONCE_BYTES {
            return Err(not_opened());
        }
        let (nonce, ciphertext) = rest.split_at(NONCE_BYTES);
        let payload = Payload {
            msg: ciphertext,
            aad: name.as_bytes(),
        };
        let value = Zeroizing::new(
            self.cipher
                .decrypt(XNonce::from_slice(nonce), payload)
                .map_err(|_| not_opened())?,
        );
        String::from_utf8(value.to_vec()).map_err(|_| not_opened())
    }
}

fn random_key() -> Result<Zeroizing<[u8; KEY_BYTES]>, String> {
    let mut key = Zeroizing::new([0_u8; KEY_BYTES]);
    getrandom::fill(key.as_mut()).map_err(|e| format!("The system gave no random bytes for a key: {e}"))?;
    Ok(key)
}

/// The key the key file at file keeps, unwrapped from what it holds.
fn key_in(file: &Path, kept: &[u8]) -> Result<Zeroizing<[u8; KEY_BYTES]>, String> {
    let unwrapped = platform::unprotect(kept).map_err(|why| format!("The key file {} {why}", file.display()))?;
    let key: [u8; KEY_BYTES] = unwrapped.as_slice().try_into().map_err(|_| {
        format!(
            "The key file {} is damaged: it holds {} bytes, not a {KEY_BYTES}-byte key.",
            file.display(),
            unwrapped.len()
        )
    })?;
    Ok(Zeroizing::new(key))
}

/// Where a new key file is written before it is linked in place: beside it, named by this
/// process, so two programs making one at once don't write the same file.
fn staging_file(file: &Path) -> PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(format!(".{}.new", std::process::id()));
    PathBuf::from(name)
}

/// Writes bytes to a new file at path that only its owner can read (on Unix; on Windows the data
/// folder's own permissions apply, and DPAPI protects the key).
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    write_with(&mut options, path, bytes)
}

/// write_private, failing when path is there already.
fn write_new_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    write_with(&mut options, path, bytes)
}

fn write_with(options: &mut std::fs::OpenOptions, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        /// Read and write for the file's owner only.
        const OWNER_ONLY: u32 = 0o600;
        options.mode(OWNER_ONLY);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Windows: the key wrapped with DPAPI for the user who runs the app.
#[cfg(windows)]
mod platform {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    };
    use zeroize::Zeroizing;

    /// key wrapped for this Windows user.
    pub fn protect(key: &[u8]) -> Result<Vec<u8>, String> {
        let input = blob_of(key);
        let mut output = CRYPT_INTEGER_BLOB::default();
        // SAFETY: input points at key for its length, and output is filled by Windows, which
        // allocates it with LocalAlloc; it is copied and freed below.
        unsafe {
            CryptProtectData(
                &input,
                windows::core::w!("JobFinder data key"),
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
        .map_err(|e| format!("Windows couldn't protect the key (DPAPI): {e}"))?;
        // SAFETY: Windows filled output with a buffer of cbData bytes.
        Ok(unsafe { taken(output) }.to_vec())
    }

    /// The key wrapped in kept; Err (said after the file's name) when this user can't unwrap it.
    pub fn unprotect(kept: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
        let input = blob_of(kept);
        let mut output = CRYPT_INTEGER_BLOB::default();
        // SAFETY: as in protect().
        unsafe {
            CryptUnprotectData(
                &input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
        .map_err(|e| {
            format!("couldn't be opened by Windows (DPAPI): {e}. It was made by another Windows user or on another computer.")
        })?;
        // SAFETY: as in protect().
        Ok(unsafe { taken(output) })
    }

    fn blob_of(bytes: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB {
            cbData: bytes.len() as u32,
            pbData: bytes.as_ptr().cast_mut(),
        }
    }

    /// blob's bytes, copied out, with its buffer wiped and given back to Windows.
    ///
    /// SAFETY: blob must be one Windows filled, with a LocalAlloc buffer of cbData bytes.
    unsafe fn taken(blob: CRYPT_INTEGER_BLOB) -> Zeroizing<Vec<u8>> {
        if blob.pbData.is_null() {
            return Zeroizing::new(Vec::new());
        }
        // SAFETY: the caller's promise.
        let bytes = unsafe { std::slice::from_raw_parts_mut(blob.pbData, blob.cbData as usize) };
        let copied = Zeroizing::new(bytes.to_vec());
        zeroize::Zeroize::zeroize(bytes);
        // SAFETY: the buffer is Windows' LocalAlloc one, freed once.
        unsafe {
            let _ = LocalFree(Some(HLOCAL(blob.pbData.cast())));
        }
        copied
    }
}

/// Elsewhere: the key itself, in a file only its owner can read.
#[cfg(not(windows))]
mod platform {
    use zeroize::Zeroizing;

    pub fn protect(key: &[u8]) -> Result<Vec<u8>, String> {
        Ok(key.to_vec())
    }

    pub fn unprotect(kept: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
        Ok(Zeroizing::new(kept.to_vec()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sealed_value_opens_under_its_own_name_only() {
        let key = DataKey::random().unwrap();
        let sealed = key.seal("llm_key:openai", "sk-one").unwrap();
        assert_eq!(key.open("llm_key:openai", &sealed).unwrap(), "sk-one");
        assert!(key.open("llm_key:anthropic", &sealed).is_err(), "moved to another name");
    }

    #[test]
    fn another_key_doesnt_open_it() {
        let sealed = DataKey::random().unwrap().seal("llm_key:openai", "sk-one").unwrap();
        let why = DataKey::random().unwrap().open("llm_key:openai", &sealed).unwrap_err();
        assert!(why.contains("another key file"), "{why}");
    }

    #[test]
    fn each_seal_has_its_own_nonce() {
        let key = DataKey::random().unwrap();
        assert_ne!(key.seal("a", "same").unwrap(), key.seal("a", "same").unwrap());
    }

    #[test]
    fn a_damaged_value_doesnt_open() {
        let key = DataKey::random().unwrap();
        let mut sealed = key.seal("a", "value").unwrap();
        *sealed.last_mut().unwrap() ^= 1;
        assert!(key.open("a", &sealed).is_err());
        assert!(key.open("a", &[SEALED_V1]).is_err(), "too short");
        assert!(key.open("a", &[]).is_err(), "empty");
    }

    #[test]
    fn the_key_file_is_made_once_and_read_back() {
        let folder = std::env::temp_dir().join(format!("jobfinder-data-key-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let database = folder.join("jobfinder.sqlite");
        let made = DataKey::of_database(&database).unwrap();
        let sealed = made.seal("a", "value").unwrap();
        let read = DataKey::of_database(&database).unwrap();
        assert_eq!(read.open("a", &sealed).unwrap(), "value");
        assert_eq!(
            std::fs::read_dir(&folder).unwrap().count(),
            1,
            "only the key file is left, no staging file"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(key_file(&database)).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[cfg(not(windows))]
    #[test]
    fn a_short_key_file_is_said_to_be_damaged() {
        let folder = std::env::temp_dir().join(format!("jobfinder-data-key-short-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let database = folder.join("jobfinder.sqlite");
        std::fs::write(key_file(&database), b"short").unwrap();
        let why = DataKey::of_database(&database).err().unwrap();
        assert!(why.contains("damaged"), "{why}");
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
