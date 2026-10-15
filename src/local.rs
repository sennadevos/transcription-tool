use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::config::models_dir;

/// (model name, title, detail). Files come from huggingface.co/ggerganov/whisper.cpp. Best choice first.
pub const MODELS: &[(&str, &str, &str)] = &[
    ("large-v3-turbo-q5_0", "Whisper large v3 turbo, compact", "574 MB, recommended"),
    ("small", "Whisper small", "487 MB, fast"),
    ("base", "Whisper base", "147 MB, faster, rougher"),
    ("tiny", "Whisper tiny", "77 MB, fastest, roughest"),
    ("medium", "Whisper medium", "1.5 GB"),
    ("large-v3-turbo", "Whisper large v3 turbo", "1.6 GB"),
    ("large-v3", "Whisper large v3", "3.1 GB, slowest, most accurate"),
];

pub fn model_path(name: &str) -> PathBuf {
    models_dir().join(format!("ggml-{name}.bin"))
}

pub fn is_downloaded(name: &str) -> bool {
    model_path(name).is_file()
}

fn download(name: &str, progress: &dyn Fn(String)) -> Result<PathBuf> {
    let path = model_path(name);
    if path.is_file() {
        return Ok(path);
    }
    std::fs::create_dir_all(models_dir()).map_err(|_| anyhow!("The model could not be saved. Is the disk full?"))?;
    let url = format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{name}.bin");
    let mut resp = crate::openrouter::agent().get(&url).call().map_err(crate::openrouter::net_error)?;
    if !resp.status().is_success() {
        return Err(anyhow!("The model could not be downloaded (error {}). Try again later.", resp.status().as_u16()));
    }
    let total = resp.body().content_length().unwrap_or(0);
    let tmp = path.with_extension("part");
    let mut out = std::fs::File::create(&tmp)?;
    let mut reader = resp.body_mut().with_config().limit(u64::MAX).reader();
    let mut buf = vec![0u8; 1 << 20];
    let (mut done, mut last_pct) = (0u64, u64::MAX);
    loop {
        let n = reader.read(&mut buf).map_err(|_| anyhow!("The download was interrupted. Check your internet connection and try again."))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n]).map_err(|_| anyhow!("The model could not be saved. Is the disk full?"))?;
        done += n as u64;
        let pct = if total > 0 { done * 100 / total } else { 0 };
        if pct != last_pct {
            last_pct = pct;
            progress(format!("Downloading model (one time): {pct}%"));
        }
    }
    out.flush()?;
    drop(out);
    std::fs::rename(&tmp, &path)?;
    Ok(path)
}

pub struct Outcome {
    pub text: String,
    /// Seconds of audio handled per second of work on this computer.
    pub speed: f64,
}

/// `eta` is called about twice a second with the estimated seconds left, once whisper has finished its first stretch of audio.
pub fn transcribe(name: &str, samples: &[f32], language: &str, progress: &dyn Fn(String), eta: &(dyn Fn(f64) + Sync), cancel: &Arc<AtomicBool>) -> Result<Outcome> {
    let path = download(name, progress)?;
    progress("Loading model…".into());
    let ctx = WhisperContext::new_with_params(path.to_str().context("bad model path")?, WhisperContextParameters::default()).map_err(|_| {
        // A damaged file would fail every time; remove it so the next run downloads a fresh copy.
        let _ = std::fs::remove_file(&path);
        anyhow!("This model file was damaged and has been removed. Try again to download it fresh.")
    })?;
    let mut state = ctx.create_state().map_err(|_| anyhow!("There is not enough memory to run this model. Try a smaller one."))?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some(if language.is_empty() { "auto" } else { language }));
    // Leave a quarter of the processor free so the computer stays usable while this runs.
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    params.set_n_threads((cores * 3 / 4).max(1) as i32);
    let stop = cancel.clone();
    params.set_abort_callback_safe(move || stop.load(Ordering::Relaxed));
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    let percent = Arc::new(AtomicI32::new(0));
    let seen = percent.clone();
    params.set_progress_callback_safe(move |p: i32| seen.store(p, Ordering::Relaxed));
    progress("Transcribing on this computer… the time left shows once the first 30 seconds are done".into());
    let started = Instant::now();
    let finished = AtomicBool::new(false);
    let result = std::thread::scope(|scope| {
        scope.spawn(|| {
            while !finished.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(500));
                let p = percent.load(Ordering::Relaxed);
                if p > 0 && p < 100 {
                    eta(started.elapsed().as_secs_f64() * (100.0 - p as f64) / p as f64);
                }
            }
        });
        let r = state.full(params, samples);
        finished.store(true, Ordering::Relaxed);
        r
    });
    if cancel.load(Ordering::Relaxed) {
        return Err(anyhow!("Stopped."));
    }
    result.map_err(|_| anyhow!("Transcribing on this computer failed. Try a smaller model."))?;
    let speed = samples.len() as f64 / crate::audio::RATE as f64 / started.elapsed().as_secs_f64().max(0.001);
    let text: Vec<String> = state.as_iter().map(|s| s.to_string().trim().to_owned()).collect();
    Ok(Outcome { text: text.join(" "), speed })
}

#[cfg(test)]
mod tests {
    /// Needs internet for the first model download. Run with TEST_AUDIO=/path/file cargo test --release -- --ignored --nocapture local_progress
    #[test]
    #[ignore]
    fn local_progress() {
        let samples = crate::audio::decode(std::path::Path::new(&std::env::var("TEST_AUDIO").expect("TEST_AUDIO"))).unwrap();
        let etas = std::sync::Mutex::new(Vec::new());
        let out = super::transcribe("tiny", &samples, "", &|m| println!("{m}"), &|r| etas.lock().unwrap().push(r), &Default::default()).unwrap();
        let etas = etas.into_inner().unwrap();
        println!("audio {:.0}s, speed {:.1}x, {} estimates, first {:?}, last {:?}", samples.len() as f64 / 16000.0, out.speed, etas.len(), etas.first(), etas.last());
        assert!(out.speed > 0.0 && !out.text.is_empty());
    }
}
