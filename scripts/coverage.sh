#!/usr/bin/env bash
# Code coverage for wsmr.
#
# Two modes, because wsmr is developed on macOS but only *runs* on Linux:
#
#   unit    — fast, native `cargo llvm-cov` of the unit tests. Runs anywhere
#             (macOS points cargo-llvm-cov at Homebrew's llvm tools). This is a
#             SUBSET: it omits all cfg(target_os="linux") code and every path
#             that needs a live systemd/D-Bus. Inner-loop signal, NOT the gate.
#
#   merged  — the authoritative >=90% gate. Builds ONE instrumented Linux binary
#             and runs BOTH the unit tests AND the Tier-B systemd integration
#             smoke against it inside a single container, then merges the
#             profiles into one whole-crate report. This is the only meaningful
#             whole-crate number: you cannot merge a macOS build's profile with a
#             Linux build's (different binaries, different instrumented regions —
#             the pidfd path only exists in the Linux build), so the merge has to
#             happen end-to-end in one Linux environment.
#
# Auto-selection (override by passing `unit` or `merged` as the first arg, or
# set WSMR_COV_MODE):
#   - already inside a container -> `inner`  (run cargo llvm-cov directly)
#   - podman available           -> `merged` (the real number)
#   - neither                    -> `unit`   (with a loud PARTIAL warning)
#
# Env:
#   WSMR_COV_MODE=unit|merged|inner   force a mode
#   WSMR_COV_FAIL_UNDER=<pct>         min line coverage for merged (default 90)
#   WSMR_COV_HTML=1                   also emit an HTML report under coverage/
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

FAIL_UNDER="${WSMR_COV_FAIL_UNDER:-90}"
OUT_DIR="$ROOT/coverage"

# --- mode detection ---------------------------------------------------------
in_container() {
  [ -f /run/.containerenv ] || [ -f /.dockerenv ] || [ -n "${container:-}" ]
}

detect_mode() {
  if in_container; then echo inner; return; fi
  if command -v podman >/dev/null 2>&1; then echo merged; return; fi
  echo unit
}

MODE="${1:-${WSMR_COV_MODE:-$(detect_mode)}}"

# --- macOS: external llvm tools --------------------------------------------
# cargo-llvm-cov needs llvm-profdata/llvm-cov. Under a Homebrew rust toolchain
# there is no rustup `llvm-tools-preview` component, so point it at Homebrew's
# llvm (its major version must match rustc's bundled LLVM — both are 22 here).
setup_macos_llvm() {
  [ "$(uname -s)" = Darwin ] || return 0
  local d
  for d in "$(brew --prefix llvm 2>/dev/null || true)/bin" \
           /opt/homebrew/opt/llvm/bin /usr/local/opt/llvm/bin; do
    if [ -x "$d/llvm-profdata" ] && [ -x "$d/llvm-cov" ]; then
      export LLVM_PROFDATA="$d/llvm-profdata" LLVM_COV="$d/llvm-cov"
      return 0
    fi
  done
  echo "WARNING: no Homebrew llvm tools found (brew install llvm); cargo-llvm-cov may fail" >&2
}

emit_extra_reports() {
  # $1 = "unit" | "merged" — only used to label output files
  mkdir -p "$OUT_DIR"
  cargo llvm-cov report --lcov --output-path "$OUT_DIR/lcov.info"
  if [ "${WSMR_COV_HTML:-}" = 1 ]; then
    cargo llvm-cov report --html --output-dir "$OUT_DIR/html"
    echo "    HTML: $OUT_DIR/html/index.html"
  fi
  echo "    lcov: $OUT_DIR/lcov.info"
}

# --- unit (native, subset) --------------------------------------------------
run_unit() {
  setup_macos_llvm
  echo "==> coverage: unit mode (native $(uname -s))"
  echo "    NOTE: SUBSET — excludes cfg(linux) + live-systemd paths; not the 90% gate."
  # show-env keeps the run, build and report on one consistent profile set so we
  # can emit lcov/html afterwards without re-running the tests.
  # shellcheck disable=SC1090
  source <(cargo llvm-cov show-env --sh)
  cargo llvm-cov clean --workspace
  cargo llvm-cov --no-report
  cargo llvm-cov report --summary-only
  emit_extra_reports unit
}

# --- inner (we're already inside the coverage container) --------------------
run_inner() {
  echo "==> coverage: inner mode (inside container) — delegating to coverage-run.sh"
  exec bash "$ROOT/tests/integration/coverage-run.sh"
}

# --- merged (the gate) ------------------------------------------------------
run_merged() {
  command -v podman >/dev/null 2>&1 || {
    echo "ERROR: merged mode needs podman (systemd-as-PID-1 container)." >&2
    echo "       Run 'scripts/coverage.sh unit' for the native subset instead." >&2
    exit 2
  }
  local IMG="wsmr-coverage"
  echo "==> building coverage image (systemd + rust toolchain)"
  podman build -t "$IMG" -f "$ROOT/Containerfile.coverage" "$ROOT"

  echo "==> booting coverage container (systemd PID 1)"
  local CID
  CID=$(podman run -d --systemd=always \
      -v "$ROOT:/workspace" \
      -v wsmr-cargo-registry:/root/.cargo/registry \
      -v wsmr-cov-target:/workspace/target \
      "$IMG")
  # shellcheck disable=SC2064
  trap "podman rm -f '$CID' >/dev/null 2>&1 || true" EXIT

  echo "==> running coverage sequence in container (build + unit + integration + report)"
  podman exec \
      -e WSMR_COV_FAIL_UNDER="$FAIL_UNDER" \
      -e WSMR_COV_HTML="${WSMR_COV_HTML:-}" \
      "$CID" bash /workspace/tests/integration/coverage-run.sh
}

case "$MODE" in
  unit)   run_unit ;;
  inner)  run_inner ;;
  merged) run_merged ;;
  *) echo "unknown mode: $MODE (expected unit|merged|inner)" >&2; exit 2 ;;
esac
