#!/usr/bin/env bash
# P4-03 scenario: cleanup after an unclean compositor exit. Starts a normal
# session, waits for readiness, then SIGKILLs the compositor process
# directly (never calling `wsmr stop`) — asserting the unit graph's own
# OnSuccess=/OnFailure=wayland-session-shutdown.target wiring tears
# everything down and restores state exactly like a graceful `wsmr stop`
# would, without wsmr's signal-handler ever being told to stop.
set -euo pipefail

WSMR="${WSMR:-/opt/wsmr-target/debug/wsmr}"
STUB="${STUB:-/opt/it/stub-compositor.sh}"
RT="/run/user/$(id -u)"

fail() { echo "FAIL: $1" >&2; exit 1; }

collect_diagnostics() {
    echo "---- diagnostics: failed units ----" >&2
    systemctl --user list-units --failed --no-legend >&2 || true
    echo "---- diagnostics: recent user journal (last 200 lines) ----" >&2
    journalctl --user -n 200 --no-pager >&2 || true
}
trap 'rc=$?; if [ "$rc" -ne 0 ]; then collect_diagnostics; fi' EXIT

# stub-compositor.sh calls "${WSMR_BIN:?}" finalize itself — needs this
# exported into the manager's activation environment the same way the
# original smoke.sh's happy path does. Set *before* the baseline snapshot
# (same reasoning as smoke.sh): it isn't something wsmr's own cleanup
# touches, so it must appear on both sides of the restoration diff.
systemctl --user set-environment WSMR_BIN="$WSMR"

echo "== capturing the pre-session activation environment baseline =="
PRE_ENV="$(systemctl --user show-environment)"

echo "== starting a normal session =="
"$WSMR" start "$STUB" >/tmp/wsmr-start.log 2>&1 &
START_PID=$!
for _ in $(seq 1 40); do
    systemctl --user is-active graphical-session.target >/dev/null 2>&1 && break
    sleep 0.5
done
[ "$(systemctl --user is-active graphical-session.target 2>&1)" = active ] \
    || fail "session never reached graphical-session.target: $(cat /tmp/wsmr-start.log)"
echo "PASS: session reached readiness normally"

WM_UNIT=$(systemctl --user list-units --no-legend 'wayland-wm@*.service' | awk '{print $1}' | head -1)
[ -n "$WM_UNIT" ] || fail "compositor unit is not active"
COMP_PID=$(systemctl --user show -p MainPID --value "$WM_UNIT")
[ -n "$COMP_PID" ] && [ "$COMP_PID" -gt 0 ] || fail "could not determine the compositor's MainPID"

echo "== killing the compositor directly with SIGKILL (never calling 'wsmr stop') =="
kill -9 "$COMP_PID"

echo "== asserting the shutdown cascade tears the whole graph down anyway =="
for _ in $(seq 1 40); do
    [ "$(systemctl --user is-active graphical-session.target 2>&1)" != active ] && break
    sleep 0.5
done
[ "$(systemctl --user is-active graphical-session.target 2>&1)" != active ] \
    || fail "graphical-session.target is still active 20s after SIGKILLing the compositor"
echo "PASS: graphical-session.target went inactive from the compositor's own OnFailure=wayland-session-shutdown.target wiring"

for _ in $(seq 1 20); do
    pgrep -x socat >/dev/null 2>&1 || break
    sleep 0.5
done
if pgrep -x socat >/dev/null 2>&1; then
    fail "socat (wayland-stub listener) is still running after the SIGKILL-triggered shutdown"
fi
echo "PASS: the compositor's cgroup (including its socat child) was fully torn down"

systemctl --user show-environment | grep -q '^WAYLAND_DISPLAY=' \
    && fail "WAYLAND_DISPLAY was not cleaned up after the unclean exit"
echo "PASS: WAYLAND_DISPLAY was unset by the shutdown cascade despite no clean 'wsmr stop'"

wait "$START_PID" 2>/dev/null || true

echo "== 'wsmr stop' after an already-torn-down session is a clean no-op =="
"$WSMR" stop || fail "'wsmr stop' exited non-zero after the session already tore itself down"
echo "PASS: 'wsmr stop' post-SIGKILL is a clean no-op"

POST_ENV="$(systemctl --user show-environment)"
[ "$PRE_ENV" = "$POST_ENV" ] || fail "activation environment was not fully restored after the unclean exit"
echo "PASS: activation environment matches the pre-session baseline after the unclean exit"

FAILED="$(systemctl --user list-units --failed --no-legend)"
[ -z "$FAILED" ] || fail "failed units present after the unclean-exit cleanup: $FAILED"
if [ -d "$RT/wsmr" ]; then
    REMAINING="$(find "$RT/wsmr" -type f -not -name 'state.lock')"
    [ -z "$REMAINING" ] || fail "stale files remain under \$XDG_RUNTIME_DIR/wsmr: $REMAINING"
fi
echo "PASS: no failed units, no stale wsmr runtime state after the unclean exit"
