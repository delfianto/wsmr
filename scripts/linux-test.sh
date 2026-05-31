#!/usr/bin/env bash
# Build + test wsmr inside a Linux Podman container (we develop on macOS).
#
# Source is live bind-mounted; the cargo registry and the Linux target dir live
# in named volumes so iteration is fast and the Linux build never collides with
# the host's macOS target/.
#
# Usage:
#   scripts/linux-test.sh                 # cargo test (all)
#   scripts/linux-test.sh some_test_name  # filter
set -euo pipefail

IMAGE="wsmr-linux-dev"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FILTER="${1:-}"

podman build -t "$IMAGE" -f "$ROOT/Containerfile" "$ROOT"

podman run --rm \
  -v "$ROOT:/workspace" \
  -v wsmr-cargo-registry:/root/.cargo/registry \
  -v wsmr-linux-target:/workspace/target \
  -e RUST_BACKTRACE=1 \
  "$IMAGE" \
  bash -lc "cargo test ${FILTER:+-- $FILTER}"
