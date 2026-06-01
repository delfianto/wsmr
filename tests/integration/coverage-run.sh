#!/usr/bin/env bash
# Runs INSIDE the coverage container (systemd as PID 1) as root.
#
# Produces the authoritative MERGED coverage number: it builds ONE instrumented
# wsmr, runs the unit tests against it (as root) AND the Tier-B integration smoke
# against the same binary (as the `tester` user under a real user manager), then
# merges both profile sets into a single whole-crate report.
#
# Everything happens in this one filesystem so the source paths the binary
# records (/workspace/src/...) resolve at report time and the cfg(linux) code is
# compiled exactly once.
set -uo pipefail

cd /workspace
FAIL_UNDER="${WSMR_COV_FAIL_UNDER:-90}"
OUT_DIR=/workspace/coverage

fail() { echo "coverage-run: $1" >&2; exit 1; }

echo "== waiting for the system manager =="
for _ in $(seq 1 60); do
  systemctl is-system-running 2>/dev/null | grep -qE "running|degraded" && break
  sleep 0.5
done

# One env (RUSTFLAGS=-Cinstrument-coverage, LLVM_PROFILE_FILE, instrumented
# target dir) shared by build + run + report.
# shellcheck disable=SC1090
source <(cargo llvm-cov show-env --sh)
cargo llvm-cov clean --workspace

echo "== building instrumented wsmr + running unit tests =="
cargo test || fail "unit tests failed"
cargo build || fail "instrumented build failed"

COV_TARGET="${CARGO_LLVM_COV_TARGET_DIR:-/workspace/target/llvm-cov-target}"
WSMR_BIN="$COV_TARGET/debug/wsmr"
[ -x "$WSMR_BIN" ] || fail "instrumented binary not found at $WSMR_BIN"

# The tester user must be able to (a) execute the instrumented binary and (b)
# drop its .profraw next to the unit profraw.
PROF_DIR="$(dirname "$LLVM_PROFILE_FILE")"
mkdir -p "$PROF_DIR"
chmod -R a+rwX "$PROF_DIR" 2>/dev/null || true
chmod -R a+rX "$COV_TARGET" 2>/dev/null || true
chmod a+rx /workspace 2>/dev/null || true

echo "== bringing up the tester user manager (linger) =="
loginctl enable-linger tester >/dev/null
UID_T=$(id -u tester)
for _ in $(seq 1 40); do
  systemctl is-active "user@${UID_T}.service" 2>/dev/null | grep -q active && break
  sleep 0.5
done

echo "== running the integration smoke against the instrumented binary (as tester) =="
# Same env the host harness (linux-integration.sh) passes, plus WSMR/STUB
# overrides that point smoke.sh at the instrumented binary, and LLVM_PROFILE_FILE
# so the binary (and its children) write profraw into the shared profile dir.
# %p (pid) keeps every invocation's file distinct.
runuser -u tester -- env \
  XDG_RUNTIME_DIR="/run/user/${UID_T}" \
  DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/${UID_T}/bus" \
  XDG_SEAT=seat0 XDG_SESSION_ID=1 XDG_CURRENT_DESKTOP=stub \
  WSMR="$WSMR_BIN" \
  STUB="/workspace/tests/integration/stub-compositor.sh" \
  LLVM_PROFILE_FILE="$LLVM_PROFILE_FILE" \
  bash /workspace/tests/integration/smoke.sh
SMOKE_RC=$?
[ "$SMOKE_RC" -eq 0 ] || echo "WARNING: integration smoke exited $SMOKE_RC (still merging coverage; will fail at the end)" >&2

echo "== merged coverage report =="
mkdir -p "$OUT_DIR"
cargo llvm-cov report --summary-only
cargo llvm-cov report --lcov --output-path "$OUT_DIR/lcov.info"
if [ "${WSMR_COV_HTML:-}" = 1 ]; then
  cargo llvm-cov report --html --output-dir "$OUT_DIR/html"
fi

echo "== enforcing the gate (>= ${FAIL_UNDER}% lines) =="
GATE_RC=0
cargo llvm-cov report --fail-under-lines "$FAIL_UNDER" || GATE_RC=$?

# A functional regression must fail the job even if coverage is fine.
if [ "$SMOKE_RC" -ne 0 ]; then
  echo "FAIL: integration smoke exited $SMOKE_RC" >&2
  exit 1
fi
exit "$GATE_RC"
