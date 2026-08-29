#!/usr/bin/env bash
# Build + lint wsmr inside a clean Debian Podman container, for the same
# reproducibility reason as linux-test.sh. Same volume strategy: a container-
# only target dir kept separate from the host's `target/`.
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
