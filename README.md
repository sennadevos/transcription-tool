# Transcription Tool

See [SPEC.md](SPEC.md).

## For users

1. Download `transcription-tool.exe` (Windows) or `transcription-tool` (Linux) and open it. On Windows, if SmartScreen warns about an unknown app: **More info → Run anyway**.
2. Click **Install**. The app then appears in the Start menu / app menu and on the desktop.
3. Open **⚙ Settings** and paste the API key you received.
4. Choose an audio file (or drag it onto the window), pick models, click **Transcribe**.

Settings → **Uninstall…** removes the app, its shortcuts, settings and downloaded models.

## Build

Requires podman. Produces `dist/transcription-tool` and `dist/transcription-tool.exe`:

```
build/build.sh
```

## Test

End-to-end against the real API (two OpenRouter models, local Whisper tiny, merge):

```
podman run --rm -e OPENROUTER_API_KEY="$(cat ~/.config/secrets/openrouter-token)" -e TEST_AUDIO=/audio/<file> \
  -v tt-cargo:/usr/local/cargo/registry -v tt-target:/src/target -v "$PWD":/src:Z -v <audio-dir>:/audio:ro,Z \
  -w /src transcription-build cargo test --release -- --ignored --nocapture
```
