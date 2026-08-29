#!/usr/bin/env bash
# Build + test wsmr inside a clean Debian Podman container — a reproducible
# environment independent of whatever's installed on the dev host, not a
# workaround for lacking one (development is Linux-only; a plain `cargo test`
# on the host already covers the same paths).
#
# Source is live bind-mounted; the cargo registry and the container's own
# target dir live in named volumes, kept separate from the host's `target/`
# so iteration here never collides with a concurrent host-side build.
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
