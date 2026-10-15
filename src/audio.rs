use std::fs::File;
use std::io::Cursor;
use std::path::Path;
use std::sync::LazyLock;

use anyhow::{Context, Result, anyhow};
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::codecs::registry::CodecRegistry;
use symphonia::core::errors::Error;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::formats::TrackType;
use symphonia::core::formats::probe::Hint;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

pub const RATE: u32 = 16_000;
pub const CHUNK_SECONDS: usize = 300;

/// Symphonia's codecs plus Opus (WhatsApp voice notes), which Symphonia lacks.
static CODECS: LazyLock<CodecRegistry> = LazyLock::new(|| {
    let mut r = CodecRegistry::new();
    symphonia::default::register_enabled_codecs(&mut r);
    r.register_audio_decoder::<symphonia_adapter_libopus::OpusDecoder>();
    r
});

fn probe(path: &Path) -> Result<Box<dyn FormatReader>> {
    let file = File::open(path).context("cannot open file")?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .context("unsupported audio format")
}

/// Length in seconds as declared by the file, without decoding it. None when the format does not say.
pub fn duration_secs(path: &Path) -> Option<f64> {
    let format = probe(path).ok()?;
    let track = format.default_track(TrackType::Audio)?;
    let (frames, base) = (track.num_frames?, track.time_base?);
    Some(frames as f64 * base.numer.get() as f64 / base.denom.get() as f64)
}

/// Decode any supported audio file to 16 kHz mono f32.
pub fn decode(path: &Path) -> Result<Vec<f32>> {
    let mut format = probe(path)?;
    let track = format.default_track(TrackType::Audio).ok_or_else(|| anyhow!("no audio track"))?;
    let track_id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or_else(|| anyhow!("no audio codec parameters"))?;
    let mut decoder = CODECS
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .context("unsupported audio codec")?;

    let mut mono = Vec::new();
    let mut src_rate = 0;
    let mut buf: Vec<f32> = Vec::new();
    while let Some(packet) = format.next_packet().context("cannot read audio")? {
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(audio) => {
                src_rate = audio.spec().rate();
                let channels = audio.spec().channels().count().max(1);
                buf.resize(audio.samples_interleaved(), 0.0);
                audio.copy_to_slice_interleaved(&mut buf);
                mono.extend(buf.chunks(channels).map(|f| f.iter().sum::<f32>() / channels as f32));
            }
            Err(Error::DecodeError(_)) => continue,
            Err(e) => return Err(e.into()),
        }
    }
    if mono.is_empty() {
        return Err(anyhow!("no audio decoded"));
    }
    Ok(resample(&mono, src_rate, RATE))
}

/// Box-filter downsampling, linear-interpolation upsampling. Good enough for speech recognition.
fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || from == 0 {
        return input.to_vec();
    }
    let ratio = from as f64 / to as f64;
    let out_len = (input.len() as f64 / ratio) as usize;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * ratio;
            if ratio > 1.0 {
                let start = pos as usize;
                let end = (((i + 1) as f64 * ratio) as usize).clamp(start + 1, input.len());
                input[start..end].iter().sum::<f32>() / (end - start) as f32
            } else {
                let j = pos as usize;
                let frac = (pos - j as f64) as f32;
                let next = input.get(j + 1).copied().unwrap_or(input[j]);
                input[j] * (1.0 - frac) + next * frac
            }
        })
        .collect()
}

/// Split into WAV-encoded chunks for upload.
pub fn wav_chunks(samples: &[f32]) -> Result<Vec<Vec<u8>>> {
    samples
        .chunks(RATE as usize * CHUNK_SECONDS)
        .map(|chunk| {
            let mut out = Cursor::new(Vec::new());
            let spec = hound::WavSpec {
                channels: 1,
                sample_rate: RATE,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            };
            let mut w = hound::WavWriter::new(&mut out, spec)?;
            for s in chunk {
                w.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)?;
            }
            w.finalize()?;
            Ok(out.into_inner())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    /// Run with TEST_AUDIO=/path/file cargo test --release -- --ignored --nocapture duration_probe
    #[test]
    #[ignore]
    fn duration_probe() {
        let path = std::path::PathBuf::from(std::env::var("TEST_AUDIO").expect("TEST_AUDIO"));
        println!("{:?}", super::duration_secs(&path));
    }
}
