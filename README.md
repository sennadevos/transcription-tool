# Transcription Tool

See [SPEC.md](SPEC.md).

## For users

1. Download `transcription-tool.exe` (Windows) or `transcription-tool-linux-x86_64` (Linux) from the [latest release](https://github.com/sennadevos/transcription-tool/releases/latest) and open it. On Windows, if SmartScreen warns about an unknown app: **More info → Run anyway**.
2. Click **Install**. The app then appears in the Start menu / app menu and on the desktop.
3. Paste the API key you received when asked. It is saved; you only need to enter a new one when it expires (a key lasts 180 days), via the prompt in the error message or Settings.
4. Choose an audio file (or drag it onto the window), pick models, click **Transcribe**.

The app checks for a newer release when it starts and offers **Update now**; it downloads, verifies and restarts by itself.

Settings → **Uninstall…** removes the app, its shortcuts, settings and downloaded models.

## Build

Requires podman. Produces `dist/transcription-tool` and `dist/transcription-tool.exe`:

```
build/build.sh
```

To release: bump `version` in `Cargo.toml`, commit and push, then run `build/release.sh`. It builds both programs and publishes them as a GitHub release, which running copies pick up.

## Test

End-to-end against the real API (two OpenRouter models, local Whisper tiny, merge):

```
podman run --rm -e OPENROUTER_API_KEY="$(cat ~/.config/secrets/openrouter-token)" -e TEST_AUDIO=/audio/<file> \
  -v tt-cargo:/usr/local/cargo/registry -v tt-target:/src/target -v "$PWD":/src:Z -v <audio-dir>:/audio:ro,Z \
  -w /src transcription-build cargo test --release -- --ignored --nocapture
```
