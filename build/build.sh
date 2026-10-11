#!/bin/sh
# Builds dist/transcription-tool (Linux) and dist/transcription-tool.exe (Windows) in podman.
set -eu
cd "$(dirname "$0")/.."

podman build -q -t transcription-build build/ >/dev/null
podman build -q --build-arg DEBIAN=trixie -t transcription-build-win build/ >/dev/null

run() {
    image=$1 target_vol=$2; shift 2
    podman run --rm -v tt-cargo:/usr/local/cargo/registry -v "$target_vol":/src/target -v "$PWD":/src:Z -w /src \
        -e CMAKE_PROJECT_INCLUDE=/src/build/mingw-lib-prefix.cmake "$image" "$@"
}

mkdir -p dist
run transcription-build tt-target sh -c 'cargo build --release && cp target/release/transcription-tool dist/'
run transcription-build-win tt-target-win sh -c \
    'cargo build --release --target x86_64-pc-windows-gnu && cp target/x86_64-pc-windows-gnu/release/transcription-tool.exe dist/'
ls -la dist
