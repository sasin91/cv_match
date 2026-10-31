//! A GGUF model file's size and its number of layers ("{architecture}.block_count" in its
//! header), which together say how many layers fit in the graphics card's memory.
//!
//! Only the header's key/value pairs are read, up to the layer count; the tensors after them are
//! not. The layout is GGUF version 2 and 3's: "GGUF", version u32, tensor count u64, pair count
//! u64, then each pair as key (u64 length + UTF-8), value type u32, value.

use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// The four bytes a GGUF file starts with.
const MAGIC: &[u8; 4] = b"GGUF";

/// The file extension of a model llama.cpp runs.
pub const EXTENSION: &str = "gguf";

/// The header key of the model's architecture ("qwen3"), which names its layer count's key.
const ARCHITECTURE_KEY: &str = "general.architecture";

/// The suffix of the layer count's key, after the architecture.
const BLOCK_COUNT_SUFFIX: &str = ".block_count";

/// Longest key or string value read, in bytes; longer ones mean a broken file.
const MAX_STRING_BYTES: u64 = 1 << 20;

/// A model file in the models folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GgufFile {
    /// Its file name, which the API key page saves as the model.
    pub name: String,
    pub path: PathBuf,
    pub bytes: u64,
    /// Its transformer layers, when the header says.
    pub layers: Option<u32>,
}

/// The GGUF files in folder, by name; none when the folder isn't there.
pub fn list(folder: &Path) -> Vec<GgufFile> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut files: Vec<GgufFile> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| is_gguf(path))
        .filter_map(|path| read(&path).ok())
        .collect();
    files.sort_by(|a, b| a.name.cmp(&b.name));
    files
}

fn is_gguf(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case(EXTENSION))
}

/// The file's name, size and, when its header says, layers. A header that can't be read leaves
/// layers None: llama.cpp says what's wrong when it loads the file.
pub fn read(path: &Path) -> io::Result<GgufFile> {
    let bytes = std::fs::metadata(path)?.len();
    let layers = File::open(path)
        .and_then(|file| layer_count(&mut BufReader::new(file)))
        .unwrap_or_else(|e| {
            eprintln!("llama: {} has no readable GGUF header: {e}", path.display());
            None
        });
    Ok(GgufFile {
        name: path.file_name().unwrap_or_default().to_string_lossy().to_string(),
        path: path.to_path_buf(),
        bytes,
        layers,
    })
}

/// A GGUF value's type, as its header numbers them.
mod value_type {
    pub const U8: u32 = 0;
    pub const I8: u32 = 1;
    pub const U16: u32 = 2;
    pub const I16: u32 = 3;
    pub const U32: u32 = 4;
    pub const I32: u32 = 5;
    pub const F32: u32 = 6;
    pub const BOOL: u32 = 7;
    pub const STRING: u32 = 8;
    pub const ARRAY: u32 = 9;
    pub const U64: u32 = 10;
    pub const I64: u32 = 11;
    pub const F64: u32 = 12;
}

/// "{architecture}.block_count" from the header, None when it has none.
pub fn layer_count(reader: &mut (impl Read + Seek)) -> io::Result<Option<u32>> {
    let mut magic = [0u8; 4];
    reader.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not a GGUF file"));
    }
    let _version = read_u32(reader)?;
    let _tensor_count = read_u64(reader)?;
    let pair_count = read_u64(reader)?;
    let mut architecture = None;
    let mut block_counts: Vec<(String, u64)> = Vec::new();
    for _ in 0..pair_count {
        let key = read_string(reader)?;
        let kind = read_u32(reader)?;
        if key == ARCHITECTURE_KEY && kind == value_type::STRING {
            architecture = Some(read_string(reader)?);
        } else if key.ends_with(BLOCK_COUNT_SUFFIX) && is_unsigned(kind) {
            block_counts.push((key, read_unsigned(reader, kind)?));
        } else {
            skip_value(reader, kind)?;
        }
        if let Some(count) = architecture
            .as_deref()
            .and_then(|name| block_count_of(name, &block_counts))
        {
            return Ok(Some(count));
        }
    }
    Ok(None)
}

/// The architecture's layer count among the block counts read so far.
fn block_count_of(architecture: &str, block_counts: &[(String, u64)]) -> Option<u32> {
    let key = format!("{architecture}{BLOCK_COUNT_SUFFIX}");
    block_counts
        .iter()
        .find(|(name, _)| *name == key)
        .and_then(|(_, count)| u32::try_from(*count).ok())
}

fn is_unsigned(kind: u32) -> bool {
    matches!(kind, value_type::U32 | value_type::U64)
}

fn read_unsigned(reader: &mut impl Read, kind: u32) -> io::Result<u64> {
    if kind == value_type::U64 {
        read_u64(reader)
    } else {
        read_u32(reader).map(u64::from)
    }
}

/// Passes over one value of kind.
fn skip_value(reader: &mut (impl Read + Seek), kind: u32) -> io::Result<()> {
    match kind {
        value_type::STRING => {
            let length = read_length(reader)?;
            skip(reader, length)
        }
        value_type::ARRAY => {
            let element = read_u32(reader)?;
            let count = read_u64(reader)?;
            match fixed_size(element) {
                Some(size) => skip(reader, size.saturating_mul(count)),
                None => (0..count).try_for_each(|_| skip_value(reader, element)),
            }
        }
        _ => match fixed_size(kind) {
            Some(size) => skip(reader, size),
            None => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unknown GGUF value type {kind}"),
            )),
        },
    }
}

/// The bytes a value of kind takes, None for strings, arrays and unknown kinds.
fn fixed_size(kind: u32) -> Option<u64> {
    match kind {
        value_type::U8 | value_type::I8 | value_type::BOOL => Some(1),
        value_type::U16 | value_type::I16 => Some(2),
        value_type::U32 | value_type::I32 | value_type::F32 => Some(4),
        value_type::U64 | value_type::I64 | value_type::F64 => Some(8),
        _ => None,
    }
}

fn skip(reader: &mut impl Seek, bytes: u64) -> io::Result<()> {
    let offset = i64::try_from(bytes).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "value too long"))?;
    reader.seek(SeekFrom::Current(offset)).map(|_| ())
}

fn read_u32(reader: &mut impl Read) -> io::Result<u32> {
    let mut bytes = [0u8; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64(reader: &mut impl Read) -> io::Result<u64> {
    let mut bytes = [0u8; 8];
    reader.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

/// A string's length, refusing one longer than MAX_STRING_BYTES.
fn read_length(reader: &mut impl Read) -> io::Result<u64> {
    let length = read_u64(reader)?;
    if length > MAX_STRING_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "string too long"));
    }
    Ok(length)
}

fn read_string(reader: &mut impl Read) -> io::Result<String> {
    let length = read_length(reader)?;
    let mut bytes = vec![0u8; length as usize];
    reader.read_exact(&mut bytes)?;
    String::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
pub mod tests {
    use std::io::Cursor;

    use super::*;

    fn string(out: &mut Vec<u8>, text: &str) {
        out.extend((text.len() as u64).to_le_bytes());
        out.extend(text.as_bytes());
    }

    /// A GGUF header: a token array, the architecture, a float, then the layer count, as
    /// llama.cpp's converters write them (the layer count after the architecture).
    pub fn header(architecture: &str, layers: u32) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        out.extend(3u32.to_le_bytes());
        out.extend(0u64.to_le_bytes());
        out.extend(4u64.to_le_bytes());
        string(&mut out, "tokenizer.ggml.tokens");
        out.extend(value_type::ARRAY.to_le_bytes());
        out.extend(value_type::STRING.to_le_bytes());
        out.extend(2u64.to_le_bytes());
        string(&mut out, "<s>");
        string(&mut out, "</s>");
        string(&mut out, ARCHITECTURE_KEY);
        out.extend(value_type::STRING.to_le_bytes());
        string(&mut out, architecture);
        string(&mut out, "general.rope_scale");
        out.extend(value_type::F32.to_le_bytes());
        out.extend(1.0f32.to_le_bytes());
        string(&mut out, &format!("{architecture}{BLOCK_COUNT_SUFFIX}"));
        out.extend(value_type::U32.to_le_bytes());
        out.extend(layers.to_le_bytes());
        out
    }

    #[test]
    fn the_layer_count_is_read_from_the_header() {
        assert_eq!(layer_count(&mut Cursor::new(header("qwen3", 36))).unwrap(), Some(36));
    }

    #[test]
    fn another_architectures_block_count_is_not_the_layer_count() {
        let mut bytes = header("qwen3", 36);
        // The pair count says 3, so the header ends before qwen3.block_count.
        bytes[16..24].copy_from_slice(&3u64.to_le_bytes());
        assert_eq!(layer_count(&mut Cursor::new(bytes)).unwrap(), None);
    }

    #[test]
    fn a_file_that_isnt_gguf_is_refused() {
        assert!(layer_count(&mut Cursor::new(b"PK\x03\x04 a zip".to_vec())).is_err());
    }
}
