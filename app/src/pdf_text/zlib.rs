//! Flate decompression. sappkit's pdfreader uses the system's zlib, which Windows doesn't ship,
//! so this uses miniz_oxide, which the app already depends on, instead.

use miniz_oxide::inflate::stream::{InflateState, inflate};
use miniz_oxide::{DataFormat, MZFlush, MZStatus};

/// The output's first size, as a multiple of the input's: content streams pack about this well.
const FIRST_GUESS: usize = 4;
/// The smallest output buffer, in bytes.
const MIN_OUTPUT: usize = 64;

/// Inflates zlib data. A stream cut short or damaged near its end, common in PDFs, keeps what
/// was inflated before the damage.
pub fn inflate_zlib(data: &[u8]) -> Option<Vec<u8>> {
    let mut state = InflateState::new_boxed(DataFormat::Zlib);
    let mut out = vec![0; data.len() * FIRST_GUESS + MIN_OUTPUT];
    let (mut input, mut written) = (data, 0);
    loop {
        if written == out.len() {
            out.resize(out.len() * 2, 0);
        }
        let step = inflate(&mut state, input, &mut out[written..], MZFlush::None);
        written += step.bytes_written;
        input = &input[step.bytes_consumed..];
        let stuck = step.bytes_written == 0 && step.bytes_consumed == 0;
        match step.status {
            Ok(MZStatus::StreamEnd) => break,
            Ok(_) if !stuck => continue,
            // Damaged, or cut short: what came before stays.
            _ => {
                if written == 0 {
                    return None;
                }
                break;
            }
        }
    }
    out.truncate(written);
    Some(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn inflates() {
        // zlib.compress(b"hello hello hello")
        let packed = [120, 156, 203, 72, 205, 201, 201, 87, 200, 64, 144, 0, 58, 46, 6, 125];
        assert_eq!(super::inflate_zlib(&packed).unwrap(), b"hello hello hello");
    }

    #[test]
    fn a_stream_cut_short_keeps_what_came_before() {
        let packed = [120, 156, 203, 72, 205, 201, 201, 87, 200, 64, 144, 0, 58, 46, 6, 125];
        let cut = super::inflate_zlib(&packed[..packed.len() - 4]).unwrap();
        assert!(b"hello hello hello".starts_with(&cut), "{cut:?}");
    }
}
