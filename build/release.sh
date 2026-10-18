#!/bin/sh
# Publishes the version in Cargo.toml as a GitHub release. Installed copies pick it up on their next start.
# Bump `version` in Cargo.toml and commit and push first.
set -eu
cd "$(dirname "$0")/.."

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
tag="v$version"
[ -z "$(git status --porcelain)" ] || { echo "Commit your changes first." >&2; exit 1; }
git fetch -q origin
[ "$(git rev-parse HEAD)" = "$(git rev-parse @{u})" ] || { echo "Push your commits first." >&2; exit 1; }
gh release view "$tag" >/dev/null 2>&1 && { echo "$tag is already released; bump the version in Cargo.toml." >&2; exit 1; }

build/build.sh
stage=$(mktemp -d)
cp dist/transcription-tool "$stage/transcription-tool-linux-x86_64"
cp dist/transcription-tool.exe "$stage/transcription-tool.exe"
gh release create "$tag" "$stage"/* --title "Transcription Tool $version" --generate-notes --target "$(git rev-parse HEAD)"
rm -rf "$stage"
