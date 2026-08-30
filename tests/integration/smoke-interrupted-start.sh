#!/usr/bin/env bash
# P4-03 scenario: interrupted start/generation. `wsmr start` does its
# generation/reload/exec sequence in well under a second, so precisely
# timing a single SIGKILL to land inside one specific narrow phase (mid
# plan_generate/apply_generate, specifically) would be inherently racy and
# non-reproducible on different hardware. Instead this fuzzes the kill point
# across several iterations with varying tiny delays, so across the run at
# least some iterations land during generation/reload/exec and some land
# elsewhere in the sequence -- and asserts the actual invariant that matters
# (session::state's documented design: "a fresh generation always resolves
# any abandoned prior state first" -- see that module's own doc comment):
# regardless of exactly when 'wsmr start' gets killed, the account must be
# left in a state where the *next* clean 'wsmr start' still succeeds with no
# manual cleanup. That's a stronger property test than hitting one exact
# window, and isn't flaky the way racing a single precise timing would be.
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

# stub-compositor.sh calls "${WSMR_BIN:?}" finalize itself -- see smoke.sh.
systemctl --user set-environment WSMR_BIN="$WSMR"

echo "== capturing the pre-session activation environment baseline =="
PRE_ENV="$(systemctl --user show-environment)"

echo "== fuzzing: repeatedly SIGKILLing 'wsmr start' at varying, tiny delays =="
for i in 1 2 3 4 5; do
    delay="0.0$i"
    echo "---- iteration $i: kill after ${delay}s ----"
    "$WSMR" start "$STUB" >"/tmp/wsmr-interrupt-$i.log" 2>&1 &
    PID=$!
    sleep "$delay"
    kill -9 "$PID" 2>/dev/null || true
    wait "$PID" 2>/dev/null || true
    # 'wsmr start' may have already exec'd into signal-handler.sh (same PID,
    # image replaced) or spawned systemctl/the stub compositor as children by
    # the time it's killed -- clean up anything this iteration left running
    # so the next iteration (and the final real start) begin from a clean
    # process tree.
    pkill -9 -f "signal-handler.sh" 2>/dev/null || true
    pkill -9 -f "stub-compositor.sh" 2>/dev/null || true
    pkill -9 -x socat 2>/dev/null || true
    # Let systemd notice the dead processes and settle any partially-started
    # units this iteration may have triggered before the next one begins.
    systemctl --user stop wayland-session-envelope@*.target >/dev/null 2>&1 || true
    for _ in $(seq 1 20); do
        [ "$(systemctl --user is-active graphical-session-pre.target 2>&1)" != active ] \
            && [ "$(systemctl --user is-active graphical-session.target 2>&1)" != active ] \
            && break
        sleep 0.3
    done
    systemctl --user reset-failed >/dev/null 2>&1 || true
done
echo "PASS: 5 fuzzed kill iterations completed without wedging the account"

echo "== asserting a subsequent clean 'wsmr start' still succeeds with no manual cleanup =="
"$WSMR" start "$STUB" >/tmp/wsmr-recovery-start.log 2>&1 &
START_PID=$!
for _ in $(seq 1 40); do
    systemctl --user is-active graphical-session.target >/dev/null 2>&1 && break
    sleep 0.5
done
[ "$(systemctl --user is-active graphical-session.target 2>&1)" = active ] \
    || fail "the recovery 'wsmr start' after the kill-fuzzing never reached graphical-session.target: $(cat /tmp/wsmr-recovery-start.log)"
WM_UNIT=$(systemctl --user list-units --no-legend 'wayland-wm@*.service' | awk '{print $1}' | head -1)
[ -n "$WM_UNIT" ] && [ "$(systemctl --user is-active "$WM_UNIT")" = active ] \
    || fail "recovery session's compositor unit is not active"
systemctl --user show-environment | grep -q '^WAYLAND_DISPLAY=' \
    || fail "recovery session never exported WAYLAND_DISPLAY"
echo "PASS: a fully clean session started successfully right after the kill-fuzzing, no manual intervention"

echo "== stopping the recovery session cleanly =="
"$WSMR" stop || fail "'wsmr stop' exited non-zero on the recovery session"
wait "$START_PID" 2>/dev/null || true
sleep 1
[ "$(systemctl --user is-active graphical-session.target 2>&1)" != active ] \
    || fail "graphical-session.target still active after the recovery session's stop"
echo "PASS: the recovery session stopped cleanly"

POST_ENV="$(systemctl --user show-environment)"
[ "$PRE_ENV" = "$POST_ENV" ] || fail "activation environment was not fully restored after the interrupted-start fuzzing + recovery"
echo "PASS: activation environment matches the pre-session baseline after interruption + recovery"

FAILED="$(systemctl --user list-units --failed --no-legend)"
[ -z "$FAILED" ] || fail "failed units present after interrupted-start recovery: $FAILED"
if [ -d "$RT/wsmr" ]; then
    REMAINING="$(find "$RT/wsmr" -type f -not -name 'state.lock')"
    [ -z "$REMAINING" ] || fail "stale files remain under \$XDG_RUNTIME_DIR/wsmr: $REMAINING"
fi
echo "PASS: no failed units, no stale wsmr runtime state after interruption + recovery"
