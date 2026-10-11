//! Price and quality scores (1-3) shown next to each model, plus our recommended picks.
//!
//! What OpenRouter gives us:
//! - Price: every model has `pricing`. For transcription it is mostly per second of audio, so the real cost per minute is derived.
//! - Quality of language models: `benchmarks.artificial_analysis.intelligence_index` (0-100) on about two thirds of them.
//! - Quality of transcription models: nothing. The table below is Artificial Analysis' public AA-WER v2
//!   speech-to-text leaderboard (artificialanalysis.ai/speech-to-text), read on 2026-10-09. Models that are not on it,
//!   or whose leaderboard row is ambiguous (several hosts with different scores), get no accuracy score.

use serde_json::Value;

#[derive(Clone, Default)]
pub struct Rating {
    /// 1 = cheap, 3 = expensive. None when OpenRouter lists no price.
    pub price: Option<u8>,
    /// 1 = weakest, 3 = best. None when no benchmark exists.
    pub brains: Option<u8>,
    pub price_note: String,
    pub brains_note: String,
}

/// Transcription models we recommend, best first. Chosen on AA-WER and price per minute (2026-10-09).
/// Ids that OpenRouter no longer lists are skipped.
pub const RECOMMENDED_STT: &[&str] = &[
    "openai/gpt-4o-mini-transcribe",
    "microsoft/mai-transcribe-2",
    "elevenlabs/scribe-v2",
    "google/gemini-3.5-transcribe",
];

/// Language models we recommend for combining transcripts: cheap, fast and strong at plain text (2026-10-09).
/// Ids that OpenRouter no longer lists are skipped. The first stays reachable through the always-current alias.
pub const RECOMMENDED_LLM: &[&str] = &[
    "~google/gemini-flash-latest",
    "anthropic/claude-haiku-5.5",
    "openai/gpt-6-luna",
    "anthropic/claude-sonnet-5.5",
];

/// AA-WER v2 word error rate in percent, lower is better.
const WER: &[(&str, f32)] = &[
    ("microsoft/mai-transcribe-2", 2.0),
    ("elevenlabs/scribe-v2", 2.2),
    ("microsoft/mai-transcribe-1.5", 2.4),
    ("google/gemini-3.5-transcribe", 2.6),
    ("mistralai/voxtral-small-24b-2507-stt", 2.8),
    ("openai/gpt-transcribe", 3.3),
    ("mistralai/voxtral-mini-transcribe", 3.6),
    ("mistralai/voxtral-mini-3b-2507", 3.8),
    ("openai/gpt-4o-transcribe", 4.0),
    ("x-ai/grok-stt-1.0", 4.0),
    ("openai/whisper-1", 4.1),
    ("google/chirp-3", 4.3),
    ("openai/gpt-4o-mini-transcribe", 4.5),
    ("nvidia/parakeet-tdt-0.6b-v3", 4.5),
    ("openai/whisper-large-v3-turbo", 4.6),
    ("deepgram/nova-3", 5.2),
];

fn num(v: &Value) -> f64 {
    v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_f64()).unwrap_or(0.0)
}

/// Estimated cost in dollars per minute of audio.
fn stt_cost_per_minute(pricing: &Value) -> Option<f64> {
    let (prompt, completion) = (num(&pricing["prompt"]), num(&pricing["completion"]));
    if prompt <= 0.0 && completion <= 0.0 {
        return None;
    }
    Some(if completion > 0.0 {
        // Token-priced (GPT-4o, Gemini): about 10 audio tokens per second in, about 200 text tokens per minute out.
        // Checked against the cost OpenRouter reported for real requests: within 15%.
        prompt * 600.0 + completion * 200.0
    } else if prompt > 0.01 {
        prompt / 60.0 // quoted per hour (Microsoft)
    } else {
        prompt * 60.0 // quoted per second
    })
}

pub fn stt_rating(id: &str, pricing: &Value) -> Rating {
    let mut r = Rating::default();
    if let Some(per_min) = stt_cost_per_minute(pricing) {
        r.price = Some(if per_min <= 0.002 { 1 } else if per_min <= 0.005 { 2 } else { 3 });
        r.price_note = format!("About ${:.2} per hour of audio", per_min * 60.0);
    }
    if let Some((_, wer)) = WER.iter().find(|(m, _)| *m == id) {
        r.brains = Some(if *wer <= 3.0 { 3 } else if *wer <= 4.5 { 2 } else { 1 });
        r.brains_note = format!("Makes about {wer}% word errors in the Artificial Analysis benchmark");
    }
    r
}

/// Cut points between brain levels, taken from the scores of all models so they stay meaningful as models change.
pub fn intelligence_cuts(mut scores: Vec<f64>) -> (f64, f64) {
    scores.sort_by(f64::total_cmp);
    if scores.is_empty() {
        return (f64::MAX, f64::MAX);
    }
    (scores[scores.len() / 3], scores[scores.len() * 2 / 3])
}

pub fn llm_score(model: &Value) -> Option<f64> {
    model["benchmarks"]["artificial_analysis"]["intelligence_index"].as_f64()
}

pub fn llm_rating(model: &Value, cuts: (f64, f64)) -> Rating {
    let mut r = Rating::default();
    let (input, output) = (num(&model["pricing"]["prompt"]), num(&model["pricing"]["completion"]));
    // Merging reads about three transcripts and writes one.
    let blended = (3.0 * input + output) / 4.0 * 1e6;
    if input > 0.0 || output > 0.0 {
        r.price = Some(if blended <= 0.5 { 1 } else if blended <= 3.0 { 2 } else { 3 });
        r.price_note = format!("${:.2} in, ${:.2} out per million tokens", input * 1e6, output * 1e6);
    } else {
        r.price_note = "Free".into();
    }
    if let Some(score) = llm_score(model) {
        r.brains = Some(if score >= cuts.1 { 3 } else if score >= cuts.0 { 2 } else { 1 });
        r.brains_note = format!("Score {score:.0} of 100 on the Artificial Analysis intelligence index");
    }
    r
}
