use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const DEFAULT_LLM: &str = "~google/gemini-flash-latest";
pub const DEFAULT_PROMPT: &str = "You receive transcripts of the same audio, each produced by a different speech-to-text model. \
Produce the single most accurate transcript: resolve disagreements by majority and context, \
fix obvious recognition errors, keep the original language and wording. \
Do not summarise, translate or add commentary. Output only the transcript.";

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Settings {
    pub api_key: String,
    pub remote_models: BTreeSet<String>,
    pub local_models: BTreeSet<String>,
    pub merge: bool,
    pub llm: String,
    pub prompt: String,
    pub language: String,
    pub install_dismissed: bool,
    /// "system", "light" or "dark".
    pub theme: String,
    /// Measured on this computer: seconds of audio each local model handles per second of work.
    pub local_speed: BTreeMap<String, f64>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            remote_models: BTreeSet::from(["openai/gpt-4o-mini-transcribe".to_string()]),
            local_models: BTreeSet::new(),
            merge: true,
            llm: DEFAULT_LLM.into(),
            prompt: DEFAULT_PROMPT.into(),
            language: String::new(),
            install_dismissed: false,
            theme: "system".into(),
            local_speed: BTreeMap::new(),
        }
    }
}

fn dirs() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from("", "", "Transcription Tool")
}

fn settings_path() -> Option<PathBuf> {
    dirs().map(|d| d.config_dir().join("settings.json"))
}

/// Where downloaded local models live.
pub fn models_dir() -> PathBuf {
    dirs()
        .map(|d| d.data_local_dir().join("models"))
        .unwrap_or_else(|| std::env::temp_dir().join("transcription-tool-models"))
}

impl Settings {
    pub fn load() -> Self {
        settings_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = settings_path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&path, json);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
        }
    }
}
