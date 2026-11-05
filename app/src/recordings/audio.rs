//! A recording's sound, made ready for whisper-cli: decoded (m4a/AAC from the phone's call
//! recorder, MP3, WAV, FLAC, Ogg Vorbis), mixed down to one channel, resampled to 16 kHz and
//! written as a 16-bit WAV, which is what whisper.cpp reads.
//!
//! Opus (WhatsApp's voice messages) isn't decoded yet: symphonia has no Opus decoder.

use std::io::Write;
use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// The sample rate whisper.cpp reads, in Hz.
pub const WHISPER_SAMPLE_RATE: u32 = 16_000;

/// The bits per sample of the WAV written for whisper.cpp.
const BITS_PER_SAMPLE: u16 = 16;

/// The largest value a 16-bit sample holds.
const SAMPLE_MAX: f32 = i16::MAX as f32;

/// Milliseconds in a second.
const MS_PER_SECOND: u64 = 1000;

/// A recording's sound as one channel of samples between -1 and 1.
#[derive(Debug, Clone, PartialEq)]
pub struct Mono {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

impl Mono {
    /// How long it lasts, in milliseconds.
    pub fn duration_ms(&self) -> i64 {
        if self.sample_rate == 0 {
            return 0;
        }
        let ms = self.samples.len() as u64 * MS_PER_SECOND / u64::from(self.sample_rate);
        i64::try_from(ms).unwrap_or(i64::MAX)
    }
}

/// Decodes the audio file at path, mixed down to one channel.
pub fn decode(path: &Path) -> Result<Mono, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("Couldn't open {}: {e}", path.display()))?;
    let source = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(extension);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, source, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| unreadable(path, &e))?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| format!("{} has no sound in it.", path.display()))?;
    let track_id = track.id;
    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| format!("{} doesn't say its sample rate.", path.display()))?;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| unreadable(path, &e))?;
    let mut samples = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(unreadable(path, &e)),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            // A damaged packet is skipped, as players do.
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(unreadable(path, &e)),
        };
        let channels = decoded.spec().channels.count().max(1);
        let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
        buffer.copy_interleaved_ref(decoded);
        samples.extend(mix_down(buffer.samples(), channels));
    }
    Ok(Mono { samples, sample_rate })
}

/// Why path couldn't be decoded, in words.
fn unreadable(path: &Path, e: &SymphoniaError) -> String {
    match e {
        SymphoniaError::Unsupported(what) => format!(
            "{} is in a sound format the app can't read yet ({what}). It reads m4a, mp3, wav, flac and Ogg Vorbis.",
            path.display()
        ),
        _ => format!("{} couldn't be read as sound: {e}", path.display()),
    }
}

/// Interleaved samples of channels channels, averaged into one.
fn mix_down(interleaved: &[f32], channels: usize) -> impl Iterator<Item = f32> + '_ {
    interleaved
        .chunks(channels)
        .map(move |frame| frame.iter().sum::<f32>() / channels as f32)
}

/// mono at to_rate, each new sample read between the two old ones around it (linear), which is
/// plenty for speech.
pub fn resample(mono: &Mono, to_rate: u32) -> Mono {
    if mono.sample_rate == to_rate || mono.samples.is_empty() || mono.sample_rate == 0 {
        return Mono {
            samples: mono.samples.clone(),
            sample_rate: to_rate,
        };
    }
    let step = f64::from(mono.sample_rate) / f64::from(to_rate);
    let count = (mono.samples.len() as f64 / step).floor() as usize;
    let last = mono.samples.len() - 1;
    let samples = (0..count)
        .map(|index| {
            let at = index as f64 * step;
            let before = at.floor() as usize;
            let after = (before + 1).min(last);
            let weight = (at - before as f64) as f32;
            mono.samples[before] * (1.0 - weight) + mono.samples[after] * weight
        })
        .collect();
    Mono {
        samples,
        sample_rate: to_rate,
    }
}

/// Writes mono to path as a 16-bit PCM WAV.
pub fn write_wav(mono: &Mono, path: &Path) -> Result<(), String> {
    let bytes = wav_bytes(mono);
    let mut file = std::fs::File::create(path).map_err(|e| format!("Couldn't write {}: {e}", path.display()))?;
    file.write_all(&bytes)
        .map_err(|e| format!("Couldn't write {}: {e}", path.display()))
}

/// mono as a WAV file's bytes: the RIFF header, then the samples.
fn wav_bytes(mono: &Mono) -> Vec<u8> {
    const CHANNELS: u16 = 1;
    const HEADER_BYTES: u32 = 36;
    const FMT_CHUNK_BYTES: u32 = 16;
    const PCM_FORMAT: u16 = 1;
    let bytes_per_sample = u32::from(BITS_PER_SAMPLE / 8);
    let data_bytes = u32::try_from(mono.samples.len())
        .unwrap_or(u32::MAX)
        .saturating_mul(bytes_per_sample);
    let mut out = Vec::with_capacity(data_bytes as usize + HEADER_BYTES as usize + 8);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(HEADER_BYTES + data_bytes).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&FMT_CHUNK_BYTES.to_le_bytes());
    out.extend_from_slice(&PCM_FORMAT.to_le_bytes());
    out.extend_from_slice(&CHANNELS.to_le_bytes());
    out.extend_from_slice(&mono.sample_rate.to_le_bytes());
    out.extend_from_slice(&(mono.sample_rate * bytes_per_sample).to_le_bytes());
    out.extend_from_slice(&(bytes_per_sample as u16).to_le_bytes());
    out.extend_from_slice(&BITS_PER_SAMPLE.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_bytes.to_le_bytes());
    for sample in &mono.samples {
        let value = (sample.clamp(-1.0, 1.0) * SAMPLE_MAX) as i16;
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stereo_is_averaged_into_one_channel() {
        let mixed: Vec<f32> = mix_down(&[1.0, 0.0, 0.5, 0.5], 2).collect();
        assert_eq!(mixed, vec![0.5, 0.5]);
    }

    #[test]
    fn a_48_khz_recording_becomes_16_khz_and_keeps_its_length() {
        let mono = Mono {
            samples: vec![0.25; 48_000],
            sample_rate: 48_000,
        };
        let resampled = resample(&mono, WHISPER_SAMPLE_RATE);
        assert_eq!(resampled.samples.len(), 16_000);
        assert_eq!(resampled.duration_ms(), 1000);
        assert!(
            resampled
                .samples
                .iter()
                .all(|sample| (*sample - 0.25).abs() < f32::EPSILON)
        );
    }

    #[test]
    fn a_written_wav_reads_back_as_the_same_sound() {
        let folder = std::env::temp_dir().join(format!("jobfinder-audio-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("tone.wav");
        let mono = Mono {
            samples: (0..1600).map(|i| if i % 2 == 0 { 0.5 } else { -0.5 }).collect(),
            sample_rate: WHISPER_SAMPLE_RATE,
        };
        write_wav(&mono, &path).unwrap();
        let read = decode(&path).unwrap();
        std::fs::remove_dir_all(&folder).unwrap();
        assert_eq!(read.sample_rate, WHISPER_SAMPLE_RATE);
        assert_eq!(read.samples.len(), 1600);
        assert!((read.samples[0] - 0.5).abs() < 0.001);
        assert!((read.samples[1] + 0.5).abs() < 0.001);
    }
}
