# Transcription Tool — Spec

- **Form:** native Rust desktop app (egui), one executable, Windows + Linux. Non-technical users.
- **Install:** first launch offers "Install": copies itself to the user's app dir, adds a Start Menu/app-menu shortcut and a desktop shortcut. No admin rights.
- **Auth:** user-entered OpenRouter API key, saved locally. Nothing embedded.
- **Input:** one audio file (file picker or drag-and-drop).
- **Models:**
  - Remote: all OpenRouter STT models, fetched live (`/models?output_modalities=transcription`), via `/audio/transcriptions`.
  - Local: whisper.cpp models, downloaded on first use.
- **Parallel:** select 1..n models; all run concurrently; each result shown separately.
- **LLM merge (default on):** transcripts → one chat model (selectable, default `~google/gemini-flash-latest`) with an editable prompt → one final transcript.
- **Options:** language hint (optional).
- **Audio:** decoded in-app, resampled to 16 kHz mono, split into 5-minute WAV chunks for remote models.
- **Output:** per-model text + merged text; copy, save as `.txt`.
- **Errors:** per-model failure shown inline; doesn't abort other models.
- **Persisted:** key, selected models, LLM, prompt, merge toggle.
