use std::sync::LazyLock;
use std::time::Duration;

use anyhow::{Result, anyhow};
use base64::Engine;
use serde_json::{Value, json};

use crate::rating::{self, Rating};

const API: &str = "https://openrouter.ai/api/v1";
const KEY_REFUSED: &str = "OpenRouter no longer accepts this key. It has probably expired.";
const KEY_NO_CREDIT: &str = "This key has run out of credit. Ask whoever gave you the key to add more.";
const KEY_NOT_ACCEPTED: &str = "That key wasn't accepted. Check that you copied all of it. It may also have expired.";
const NO_NETWORK: &str = "Can't reach the internet. Check your connection and try again.";
const TOO_SLOW: &str = "The server took too long to answer. Try again in a moment.";

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

/// Turns a transport failure into a sentence a non-technical user can act on.
pub fn net_error(e: ureq::Error) -> anyhow::Error {
    anyhow!(match e {
        ureq::Error::Timeout(_) => TOO_SLOW.to_string(),
        ureq::Error::Io(_) | ureq::Error::HostNotFound | ureq::Error::ConnectionFailed => NO_NETWORK.to_string(),
        other => format!("Something went wrong while contacting the server ({other}). Try again."),
    })
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

/// What the user sees for a failed request. Technical detail is left out unless it is all we know.
fn status_message(status: u16, body: &Value) -> String {
    match status {
        401 => KEY_REFUSED.into(),
        402 => KEY_NO_CREDIT.into(),
        408 | 504 | 524 => TOO_SLOW.into(),
        413 => "This recording is too large for this model. Try another model.".into(),
        429 => "OpenRouter is busy right now. Wait a minute and try again.".into(),
        500..=599 => format!("OpenRouter, or the company behind this model, has a temporary problem (error {status}). Try again in a few minutes, or pick another model."),
        _ => match body["error"]["message"].as_str().map(str::trim).filter(|m| !m.is_empty()) {
            Some(m) => format!("OpenRouter refused the request: {}", m.chars().take(200).collect::<String>()),
            None => format!("OpenRouter refused the request (error {status})."),
        },
    }
}

fn check(mut resp: ureq::http::Response<ureq::Body>) -> Result<Value> {
    let status = resp.status().as_u16();
    let text = resp.body_mut().with_config().limit(64 * 1024 * 1024).read_to_string().map_err(net_error)?;
    let body: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if !(200..300).contains(&status) {
        return Err(anyhow!(status_message(status, &body)));
    }
    if body.is_null() {
        return Err(anyhow!("OpenRouter sent an answer that could not be read. Try again."));
    }
    // Some upstream failures come back as a normal answer that holds an error.
    if body["error"].is_object() {
        let code = body["error"]["code"].as_u64().unwrap_or(500) as u16;
        return Err(anyhow!(status_message(if code < 400 { 500 } else { code }, &body)));
    }
    Ok(body)
}

fn get(url: &str) -> Result<Value> {
    check(agent().get(url).call().map_err(net_error)?)
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
            .send_json(body)
            .map_err(net_error)?,
    )
}

/// Asks OpenRouter whether a key still works, without spending anything.
pub fn check_key(key: &str) -> Result<()> {
    let resp = agent().get(format!("{API}/key")).header("Authorization", format!("Bearer {}", key.trim())).call().map_err(net_error)?;
    check(resp).map(|_| ())
}

/// Same check for a key the user has just typed, where a refusal most likely means a copy mistake.
pub fn verify_key(key: &str) -> Result<()> {
    check_key(key).map_err(|e| if e.to_string() == KEY_REFUSED { anyhow!(KEY_NOT_ACCEPTED) } else { e })
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
    use super::*;

    /// Run with --network=none: cargo test --release -- --ignored offline_message
    #[test]
    #[ignore]
    fn offline_message() {
        assert_eq!(check_key("sk-or-test").unwrap_err().to_string(), NO_NETWORK);
        assert_eq!(catalog().err().unwrap().to_string(), NO_NETWORK);
    }

    /// Needs internet; the key is deliberately invalid.
    #[test]
    #[ignore]
    fn bad_key_message() {
        assert_eq!(check_key("sk-or-v1-0000").unwrap_err().to_string(), KEY_REFUSED);
        assert_eq!(verify_key("sk-or-v1-0000").unwrap_err().to_string(), KEY_NOT_ACCEPTED);
    }

    #[test]
    fn friendly_messages() {
        let none = Value::Null;
        assert!(is_key_problem(&status_message(401, &none)));
        assert!(is_key_problem(&status_message(402, &none)));
        assert!(!is_key_problem(&status_message(429, &none)));
        assert!(status_message(502, &none).contains("temporary problem"));
        assert!(status_message(400, &json!({"error": {"message": "Bad audio"}})).ends_with("Bad audio"));
        assert!(status_message(418, &none).contains("418"));
    }

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
