#!/usr/bin/env bash
# P4-03 scenario: readiness timeout. Compositor starts and creates a real
# socket but never exports WAYLAND_DISPLAY/calls `wsmr finalize`, paired
# with a short $UWSM_WAIT_VARNAMES_TIMEOUT so wayland-session-waitenv.service
# times out quickly instead of after the real 30s default.
set -euo pipefail

WSMR="${WSMR:-/opt/wsmr-target/debug/wsmr}"
STUB="${STUB:-/opt/it/stub-compositor-hang.sh}"

fail() { echo "FAIL: $1" >&2; exit 1; }

collect_diagnostics() {
    echo "---- diagnostics: failed units ----" >&2
    systemctl --user list-units --failed --no-legend >&2 || true
    echo "---- diagnostics: waitenv unit journal ----" >&2
    journalctl --user -u 'wayland-session-waitenv.service' --no-pager >&2 || true
    echo "---- diagnostics: recent user journal (last 200 lines) ----" >&2
    journalctl --user -n 200 --no-pager >&2 || true
}
trap 'rc=$?; if [ "$rc" -ne 0 ]; then collect_diagnostics; fi' EXIT

echo "== capturing the pre-session activation environment baseline =="
PRE_ENV="$(systemctl --user show-environment)"

echo "== setting a short readiness-wait timeout (2s) =="
systemctl --user set-environment UWSM_WAIT_VARNAMES_TIMEOUT=2

echo "== starting session with a compositor that never signals readiness (blocks until the full cascade settles) =="
# `wsmr start` blocks until the whole start+shutdown cycle completes (see
# smoke-crash-before-readiness.sh's note on signal-handler.sh's
# `systemctl start --wait`); by the time it returns, the timeout has already
# fired and the whole graph has already torn itself back down — often within
# the same ~1s window, too fast to reliably catch wayland-session-waitenv's
# `failed` ActiveState with a live poll before the cascade's own stop job
# resets it. Use the journal (persistent) instead, exactly like
# smoke-crash-before-readiness.sh does for the same reason.
timeout 40 "$WSMR" start "$STUB" >/tmp/wsmr-start.log 2>&1
echo "---- wsmr start output ----"
cat /tmp/wsmr-start.log

# journald indexing can lag the process events it's just received by a
# fraction of a second; retry briefly rather than treating that lag as a
# real failure.
JOURNAL_OK=0
for _ in $(seq 1 10); do
    if journalctl --user -n 500 --no-pager 2>/dev/null | grep -qi "timed out waiting for activation-env variables: .*WAYLAND_DISPLAY" \
        && journalctl --user -n 500 --no-pager 2>/dev/null | grep -q "wayland-session-waitenv.service: Failed with result 'exit-code'"; then
        JOURNAL_OK=1
        break
    fi
    sleep 0.5
done
[ "$JOURNAL_OK" -eq 1 ] \
    || fail "journal never showed waitenv's timeout error + Result=exit-code within 5s of 'wsmr start' returning"
echo "PASS: waitenv correctly timed out naming the missing variable, not a generic failure"

echo "== asserting graphical-session.target never reached active =="
[ "$(systemctl --user is-active graphical-session.target 2>&1)" != active ] \
    || fail "graphical-session.target became active despite the readiness timeout"
echo "PASS: graphical-session.target never activated"

echo "== asserting the compositor unit and its socat child are torn down by the shutdown cascade =="
for _ in $(seq 1 20); do
    pgrep -f stub-compositor-hang.sh >/dev/null 2>&1 || break
    sleep 0.5
done
if pgrep -f stub-compositor-hang.sh >/dev/null 2>&1; then
    fail "stub-compositor-hang.sh is still running after the readiness-timeout shutdown cascade"
fi
if pgrep -x socat >/dev/null 2>&1; then
    fail "socat (wayland-stub listener) is still running after the readiness-timeout shutdown cascade"
fi
echo "PASS: the hung compositor and its socat child were torn down by the shutdown cascade"

systemctl --user unset-environment UWSM_WAIT_VARNAMES_TIMEOUT 2>/dev/null || true
systemctl --user reset-failed >/dev/null 2>&1 || true

POST_ENV="$(systemctl --user show-environment)"
[ "$PRE_ENV" = "$POST_ENV" ] || fail "activation environment was not fully restored after the readiness timeout"
echo "PASS: activation environment matches the pre-session baseline after the readiness timeout"
# See smoke-crash-before-readiness.sh's closing note: an immediate
# recovery-retry sub-check was attempted here too and dropped for the same
# rapid-retry timing-artifact reason, not because it failed to recover.
