use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::Sender;

use crate::{audio, local, openrouter};

pub enum Event {
    Status(String),
    Progress { id: String, msg: String },
    /// How fast a local model ran on this computer, for estimating the next run.
    LocalSpeed { model: String, speed: f64 },
    Finished { id: String, result: Result<String, String> },
    Merged(Result<String, String>),
    /// The file could not be read as audio on this computer.
    UnreadableFile,
    Done,
}

pub struct Job {
    pub file: PathBuf,
    pub key: String,
    pub language: String,
    /// (id, label). Local ids are prefixed with `local/`.
    pub models: Vec<(String, String)>,
    pub merge: bool,
    pub llm: String,
    pub prompt: String,
    /// Set to stop local transcription; online requests finish by themselves.
    pub cancel: Arc<AtomicBool>,
}

/// OpenRouter accepts these as-is when we cannot decode the file ourselves.
const PASSTHROUGH: &[&str] = &["wav", "mp3", "flac", "m4a", "ogg", "webm", "aac", "opus"];
const PASSTHROUGH_LIMIT: u64 = 25 * 1024 * 1024;

/// A local run expected to take longer than this gets a warning while it runs.
const SLOW_SECS: f64 = 300.0;

/// "less than a minute", "about 4 min", "about 1 h 20 min".
pub fn human_duration(secs: f64) -> String {
    let mins = (secs / 60.0).ceil() as u64;
    match mins {
        0 | 1 if secs < 45.0 => "less than a minute".into(),
        0 | 1 => "about 1 min".into(),
        2..=89 => format!("about {mins} min"),
        _ => format!("about {} h {} min", mins / 60, mins % 60),
    }
}

const CANT_READ: &str = "This file can't be opened as audio. Try an mp3, wav or m4a file.";
const CANT_PREPARE: &str = "This recording could not be prepared for sending. Try another file.";
const NO_TEXT: &str = "This model found no speech. The recording may be silent, or the model may not support this language.";

pub fn run(job: Job, tx: Sender<Event>, ctx: eframe::egui::Context) {
    let send = |e: Event| {
        let _ = tx.send(e);
        ctx.request_repaint();
    };

    send(Event::Status("Reading audio…".into()));
    let decoded = audio::decode(&job.file);
    if decoded.is_err() {
        send(Event::UnreadableFile);
    }
    let has_remote = job.models.iter().any(|(id, _)| !id.starts_with("local/"));

    // Remote payload: decoded WAV chunks, or the original file if we could not decode it.
    let remote_parts: Result<Vec<(Vec<u8>, String)>, String> = if !has_remote {
        Ok(Vec::new())
    } else {
        match &decoded {
            Ok(samples) => audio::wav_chunks(samples)
                .map(|c| c.into_iter().map(|b| (b, "wav".to_string())).collect())
                .map_err(|_| CANT_PREPARE.to_string()),
            Err(_) => passthrough(&job.file).ok_or_else(|| CANT_READ.to_string()),
        }
    };
    if let Ok(parts) = &remote_parts {
        if parts.len() > 1 {
            send(Event::Status(format!("Long recording: split into {} parts", parts.len())));
        }
    }
    send(Event::Status("Transcribing…".into()));

    let results: Mutex<Vec<(usize, String, String)>> = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for (i, (id, label)) in job.models.iter().enumerate() {
            let (send, results, job, decoded, remote_parts) = (&send, &results, &job, &decoded, &remote_parts);
            s.spawn(move || {
                let progress = |msg: String| send(Event::Progress { id: id.clone(), msg });
                let result = if let Some(name) = id.strip_prefix("local/") {
                    match decoded {
                        Ok(samples) => {
                            let eta = |remaining: f64| {
                                let mut msg = format!("Transcribing on this computer… {} left", human_duration(remaining));
                                if remaining > SLOW_SECS {
                                    msg.push_str("\nThis is slow on this computer. Online models are usually much quicker.");
                                }
                                progress(msg);
                            };
                            local::transcribe(name, samples, &job.language, &progress, &eta, &job.cancel)
                                .map(|o| {
                                    // Short clips are dominated by fixed costs, so they say little about speed.
                                    if samples.len() >= 60 * audio::RATE as usize {
                                        send(Event::LocalSpeed { model: name.to_string(), speed: o.speed });
                                    }
                                    o.text
                                })
                                .map_err(|e| e.to_string())
                        }
                        Err(_) => Err("This file type can't be transcribed on this computer. Try an online model.".to_string()),
                    }
                } else {
                    remote_parts.as_ref().map_err(Clone::clone).and_then(|parts| {
                        let mut texts = Vec::new();
                        for (n, (bytes, fmt)) in parts.iter().enumerate() {
                            if parts.len() > 1 {
                                progress(format!("Part {} of {}…", n + 1, parts.len()));
                            }
                            texts.push(
                                openrouter::transcribe(&job.key, id, bytes, fmt, &job.language).map_err(|e| e.to_string())?,
                            );
                        }
                        Ok(texts.join("\n"))
                    })
                };
                let result = result.and_then(|t| if t.trim().is_empty() { Err(NO_TEXT.to_string()) } else { Ok(t) });
                if let Ok(text) = &result {
                    results.lock().unwrap().push((i, label.clone(), text.clone()));
                }
                send(Event::Finished { id: id.clone(), result });
            });
        }
    });

    let mut results = results.into_inner().unwrap();
    results.sort_by_key(|r| r.0);
    if job.merge && !results.is_empty() {
        send(Event::Status("Combining the transcripts…".into()));
        let body = results
            .iter()
            .enumerate()
            .map(|(n, (_, label, text))| format!("### Transcript {} ({label})\n{text}", n + 1))
            .collect::<Vec<_>>()
            .join("\n\n");
        let merged = openrouter::chat(&job.key, &job.llm, &job.prompt, &body).map_err(|e| e.to_string());
        send(Event::Merged(merged));
    }
    send(Event::Done);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_naturally() {
        assert_eq!(human_duration(10.0), "less than a minute");
        assert_eq!(human_duration(50.0), "about 1 min");
        assert_eq!(human_duration(200.0), "about 4 min");
        assert_eq!(human_duration(5400.0), "about 1 h 30 min");
    }

    /// End-to-end against the real API. Run with:
    /// OPENROUTER_API_KEY=… TEST_AUDIO=/path/file cargo test --release -- --ignored --nocapture
    #[test]
    #[ignore]
    fn end_to_end() {
        let (tx, rx) = std::sync::mpsc::channel();
        let job = Job {
            file: std::env::var("TEST_AUDIO").expect("TEST_AUDIO").into(),
            key: std::env::var("OPENROUTER_API_KEY").expect("OPENROUTER_API_KEY"),
            language: String::new(),
            models: vec![
                ("openai/gpt-4o-mini-transcribe".into(), "gpt-4o-mini".into()),
                ("qwen/qwen3-asr-0.6b".into(), "qwen".into()),
                ("local/tiny".into(), "local tiny".into()),
            ],
            merge: true,
            llm: crate::config::DEFAULT_LLM.into(),
            prompt: crate::config::DEFAULT_PROMPT.into(),
            cancel: Default::default(),
        };
        run(job, tx, eframe::egui::Context::default());
        let mut failures = 0;
        for ev in rx {
            match ev {
                Event::Finished { id, result } => {
                    println!("{id}: {result:?}");
                    failures += result.is_err() as u32;
                }
                Event::Merged(r) => {
                    println!("MERGED: {r:?}");
                    failures += r.is_err() as u32;
                }
                _ => {}
            }
        }
        assert_eq!(failures, 0);
    }
}

fn passthrough(file: &PathBuf) -> Option<Vec<(Vec<u8>, String)>> {
    let ext = file.extension()?.to_str()?.to_lowercase();
    let fmt = if ext == "opus" { "ogg".to_string() } else { ext };
    if !PASSTHROUGH.contains(&fmt.as_str()) || std::fs::metadata(file).ok()?.len() > PASSTHROUGH_LIMIT {
        return None;
    }
    Some(vec![(std::fs::read(file).ok()?, fmt)])
}
