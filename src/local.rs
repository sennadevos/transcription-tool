use std::io::{Read, Write};
use std::path::PathBuf;

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
    std::fs::create_dir_all(models_dir())?;
    let url = format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{name}.bin");
    let mut resp = crate::openrouter::agent().get(&url).call()?;
    if !resp.status().is_success() {
        return Err(anyhow!("Model download failed: HTTP {}", resp.status()));
    }
    let total = resp.body().content_length().unwrap_or(0);
    let tmp = path.with_extension("part");
    let mut out = std::fs::File::create(&tmp)?;
    let mut reader = resp.body_mut().with_config().limit(u64::MAX).reader();
    let mut buf = vec![0u8; 1 << 20];
    let (mut done, mut last_pct) = (0u64, u64::MAX);
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
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

pub fn transcribe(name: &str, samples: &[f32], language: &str, progress: &dyn Fn(String)) -> Result<String> {
    let path = download(name, progress)?;
    progress("Loading model…".into());
    let ctx = WhisperContext::new_with_params(path.to_str().context("bad model path")?, WhisperContextParameters::default())
        .map_err(|e| anyhow!("Cannot load model: {e}"))?;
    let mut state = ctx.create_state().map_err(|e| anyhow!("{e}"))?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some(if language.is_empty() { "auto" } else { language }));
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    params.set_n_threads(threads as i32);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    progress("Transcribing on this computer…".into());
    state.full(params, samples).map_err(|e| anyhow!("{e}"))?;
    let text: Vec<String> = state.as_iter().map(|s| s.to_string().trim().to_owned()).collect();
    Ok(text.join(" "))
}
