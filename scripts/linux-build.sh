#!/usr/bin/env bash
# Build + lint wsmr for Linux inside a Podman container. Same volume strategy as
# linux-test.sh (separate Linux target dir from the host macOS target/).
set -euo pipefail

IMAGE="wsmr-linux-dev"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

podman build -t "$IMAGE" -f "$ROOT/Containerfile" "$ROOT"

podman run --rm \
  -v "$ROOT:/workspace" \
  -v wsmr-cargo-registry:/root/.cargo/registry \
  -v wsmr-linux-target:/workspace/target \
  "$IMAGE" \
  bash -lc "cargo build --all-targets && cargo clippy --all-targets -- -D warnings"
