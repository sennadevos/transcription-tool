use std::sync::LazyLock;
use std::time::Duration;

use anyhow::{Result, anyhow};
use base64::Engine;
use serde_json::{Value, json};

use crate::rating::{self, Rating};

const API: &str = "https://openrouter.ai/api/v1";
const KEY_REFUSED: &str = "The API key was refused";
const KEY_NO_CREDIT: &str = "This API key has no credit left";

/// True when an error means the user has to look at their key again.
pub fn is_key_problem(msg: &str) -> bool {
    msg.contains(KEY_REFUSED) || msg.contains(KEY_NO_CREDIT) || msg.starts_with("No API key")
}

static AGENT: LazyLock<ureq::Agent> = LazyLock::new(|| {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(900)))
        .build()
        .into()
});

pub fn agent() -> &'static ureq::Agent {
    &AGENT
}

#[derive(Clone)]
pub struct SttModel {
    pub id: String,
    /// Display name without the provider prefix.
    pub name: String,
    pub provider: String,
    pub created: i64,
    pub rating: Rating,
}

#[derive(Clone)]
pub struct LlmModel {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub rating: Rating,
}

pub struct Catalog {
    pub stt: Vec<SttModel>,
    pub llm: Vec<LlmModel>,
}

fn split_name(m: &Value) -> (String, String) {
    let full = m["name"].as_str().unwrap_or_default();
    let (provider, name) = full.split_once(": ").unwrap_or(("", full));
    (provider.to_string(), name.to_string())
}

fn check(mut resp: ureq::http::Response<ureq::Body>) -> Result<Value> {
    let status = resp.status();
    let body: Value = resp.body_mut().with_config().limit(64 * 1024 * 1024).read_json()?;
    if !status.is_success() {
        let msg = body["error"]["message"].as_str().map(str::to_owned).unwrap_or_else(|| body.to_string());
        return Err(anyhow!(match status.as_u16() {
            401 => format!("{KEY_REFUSED} ({msg})."),
            402 => format!("{KEY_NO_CREDIT} ({msg})."),
            _ => format!("HTTP {status}: {msg}"),
        }));
    }
    Ok(body)
}

fn get(url: &str) -> Result<Value> {
    check(agent().get(url).call()?)
}

fn post(path: &str, key: &str, body: &Value) -> Result<Value> {
    if key.trim().is_empty() {
        return Err(anyhow!("No API key set."));
    }
    check(
        agent()
            .post(format!("{API}{path}"))
            .header("Authorization", format!("Bearer {}", key.trim()))
            .header("X-Title", "Transcription Tool")
            .send_json(body)?,
    )
}

/// Checks a key against OpenRouter without spending anything.
pub fn verify_key(key: &str) -> Result<()> {
    check(agent().get(format!("{API}/key")).header("Authorization", format!("Bearer {}", key.trim())).call()?).map(|_| ())
}

/// Transcription models: those with usage data first, most used on top, then the rest newest first.
pub fn catalog() -> Result<Catalog> {
    let stt = get(&format!("{API}/models?output_modalities=transcription&sort=most-popular"))?;
    let all = get(&format!("{API}/models?sort=most-popular"))?;
    let mut stt: Vec<SttModel> = stt["data"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| {
            let (provider, name) = split_name(m);
            let id = m["id"].as_str().unwrap_or_default().to_string();
            SttModel { rating: rating::stt_rating(&id, &m["pricing"]), id, name, provider, created: m["created"].as_i64().unwrap_or(0) }
        })
        .collect();
    // Models without usage data come back sorted by id. Find that alphabetical tail and re-sort it by release date.
    let mut tail = stt.len();
    while tail > 1 && stt[tail - 2].id <= stt[tail - 1].id {
        tail -= 1;
    }
    stt[tail - 1..].sort_by_key(|m| std::cmp::Reverse(m.created));
    let text_models: Vec<&Value> = all["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["architecture"]["output_modalities"].as_array().is_some_and(|o| o.iter().any(|x| x == "text")))
        .filter(|m| !m["id"].as_str().unwrap_or_default().ends_with(":batch"))
        .collect();
    let cuts = rating::intelligence_cuts(text_models.iter().filter_map(|m| rating::llm_score(m)).collect());
    let llm = text_models
        .iter()
        .map(|m| {
            let (provider, name) = split_name(m);
            LlmModel { id: m["id"].as_str().unwrap_or_default().into(), name, provider, rating: rating::llm_rating(m, cuts) }
        })
        .collect();
    Ok(Catalog { stt, llm })
}

pub fn transcribe(key: &str, model: &str, audio: &[u8], format: &str, language: &str) -> Result<String> {
    let mut body = json!({
        "model": model,
        "input_audio": {
            "data": base64::engine::general_purpose::STANDARD.encode(audio),
            "format": format,
        },
    });
    if !language.is_empty() {
        body["language"] = json!(language);
    }
    let resp = post("/audio/transcriptions", key, &body)?;
    Ok(resp["text"].as_str().unwrap_or_default().trim().to_owned())
}

pub fn chat(key: &str, model: &str, system: &str, user: &str) -> Result<String> {
    let resp = post(
        "/chat/completions",
        key,
        &json!({
            "model": model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
        }),
    )?;
    resp["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.trim().to_owned())
        .ok_or_else(|| anyhow!("Empty answer from {model}"))
}

#[cfg(test)]
mod tests {
    /// Hits the public model list. Run with: cargo test --release -- --ignored --nocapture
    #[test]
    #[ignore]
    fn catalog_order() {
        let c = super::catalog().unwrap();
        assert!(!c.stt.is_empty() && !c.llm.is_empty());
        for m in &c.stt {
            println!("{} {}", m.created, m.id);
        }
    }
}
