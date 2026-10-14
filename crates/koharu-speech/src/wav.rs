use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WavFormat {
    pub sample_rate: u32,
    pub duration: Duration,
}

/// Reads the RIFF header of a PCM or float WAV file.
pub fn format(bytes: &[u8]) -> Result<WavFormat> {
    ensure!(
        bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WAVE",
        "speech audio is not a WAV file"
    );
    let mut cursor = 12;
    let mut fmt = None;
    while cursor + 8 <= bytes.len() {
        let id = &bytes[cursor..cursor + 4];
        let declared = u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into()?) as usize;
        let body = cursor + 8;
        // Streaming writers may leave the data size unset; clamp to the bytes present.
        let size = declared.min(bytes.len() - body);
        match id {
            b"fmt " => {
                ensure!(size >= 16, "WAV format chunk is truncated");
                let chunk = &bytes[body..body + 16];
                let channels = u16::from_le_bytes(chunk[2..4].try_into()?);
                let sample_rate = u32::from_le_bytes(chunk[4..8].try_into()?);
                let byte_rate = u32::from_le_bytes(chunk[8..12].try_into()?);
                ensure!(
                    channels > 0 && sample_rate > 0 && byte_rate > 0,
                    "WAV format chunk is invalid"
                );
                fmt = Some((sample_rate, byte_rate));
            }
            b"data" => {
                let (sample_rate, byte_rate) =
                    fmt.context("WAV data chunk precedes its format chunk")?;
                return Ok(WavFormat {
                    sample_rate,
                    duration: Duration::from_secs_f64(size as f64 / f64::from(byte_rate)),
                });
            }
            _ => {}
        }
        cursor = body + size + (size & 1);
    }
    bail!("WAV file has no data chunk")
}

#[cfg(test)]
fn pcm16(sample_rate: u32, samples: &[i16]) -> Vec<u8> {
    let data = samples.len() as u32 * 2;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_pcm_duration() {
        let wav = pcm16(44_100, &vec![0; 22_050]);
        assert_eq!(
            format(&wav).unwrap(),
            WavFormat {
                sample_rate: 44_100,
                duration: Duration::from_millis(500),
            }
        );
    }

    #[test]
    fn clamps_unset_streaming_data_size() {
        let mut wav = pcm16(10, &[0; 10]);
        wav[40..44].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(format(&wav).unwrap().duration, Duration::from_secs(1));
    }

    #[test]
    fn rejects_non_wav_bytes() {
        assert!(format(b"ID3\x04 not a wav").is_err());
    }
}
