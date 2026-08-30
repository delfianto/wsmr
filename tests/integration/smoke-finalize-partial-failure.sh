#!/usr/bin/env bash
# P4-03 scenario: finalize partial failure. `wsmr aux finalize` does two
# things in sequence: (1) export WAYLAND_DISPLAY/DISPLAY/extra_vars into the
# systemd (and dbus-broker) activation environment, recording them for
# cleanup; (2) exec into `systemd-notify` to signal readiness. This scenario
# breaks step (2) only (a shadowed, always-failing systemd-notify -- see
# stub-compositor-badnotify.sh), so step (1)'s effect (the exported
# variables) is real and observable even though the compositor's own process
# then dies -- the actual "partial failure" this scenario name describes,
# distinct from the already-fixed $NOTIFY_SOCKET-context bug from the
# original Phase 4 work.
set -euo pipefail

# Force plain output from every systemd tool this script calls: journalctl
# colorizes elevated-priority lines (systemd logs a unit's own "Failed with
# result" line at LOG_WARNING, unlike its routine LOG_INFO state-transition
# lines) whenever it thinks its output is going to a color-capable
# terminal -- which can happen even through a pipe, depending on how the
# calling environment's own stdout is set up (observed: a plain-substring
# grep against exactly that one elevated-priority line failed consistently
# on GitHub-hosted runners, every single retry across a 6s window, while an
# adjacent grep against a routine info-level line in the same journal
# succeeded immediately -- never reproduced locally).
export SYSTEMD_COLORS=0

WSMR="${WSMR:-/opt/wsmr-target/debug/wsmr}"
STUB="${STUB:-/opt/it/stub-compositor-badnotify.sh}"

fail() { echo "FAIL: $1" >&2; exit 1; }

collect_diagnostics() {
    echo "---- diagnostics: failed units ----" >&2
    systemctl --user list-units --failed --no-legend >&2 || true
    echo "---- diagnostics: recent user journal (last 200 lines) ----" >&2
    journalctl --user -n 200 --no-pager >&2 || true
}
trap 'rc=$?; if [ "$rc" -ne 0 ]; then collect_diagnostics; fi' EXIT

# stub-compositor-badnotify.sh calls "${WSMR_BIN:?}" finalize itself.
systemctl --user set-environment WSMR_BIN="$WSMR"

echo "== capturing the pre-session activation environment baseline =="
PRE_ENV="$(systemctl --user show-environment)"

echo "== starting session with a compositor whose finalize->systemd-notify step is broken =="
set +e
timeout 30 "$WSMR" start "$STUB" >/tmp/wsmr-start.log 2>&1
RC=$?
set -e
echo "---- wsmr start exit code: $RC ----"
echo "---- wsmr start output ----"
cat /tmp/wsmr-start.log
[ "$RC" -ne 124 ] || fail "'wsmr start' hung for 30s instead of surfacing the finalize failure"
[ "$RC" -eq 0 ] \
    || fail "'wsmr start' exited $RC, diverging from the confirmed exit-0-on-this-failure-mode behavior"

JOURNAL_OK=0
JOURNAL_SNAPSHOT=""
for _ in $(seq 1 10); do
    JOURNAL_SNAPSHOT="$(journalctl --user -n 500 --no-pager 2>/dev/null || true)"
    if printf '%s\n' "$JOURNAL_SNAPSHOT" | grep -q "fake systemd-notify deliberately failing" \
        && printf '%s\n' "$JOURNAL_SNAPSHOT" | grep -q "wayland-wm@.*\.service: Main process exited"; then
        JOURNAL_OK=1
        break
    fi
    sleep 0.5
done
if [ "$JOURNAL_OK" -ne 1 ]; then
    echo "---- debug: last journal snapshot checked ----" >&2
    printf '%s\n' "$JOURNAL_SNAPSHOT" >&2
    fail "journal never showed both the fake notify's failure and the compositor unit's exit within 5s"
fi
echo "PASS: journal confirms finalize's notify step actually ran and failed, killing the compositor's own process"

echo "== asserting the compositor unit itself ended up failed, not stuck 'activating' =="
# The journal, not live systemd state, is the only usable source of truth
# here: wayland-wm@.service carries CollectMode=inactive-or-failed (see
# templates.rs), so systemd forgets the unit almost immediately once it
# fails -- confirmed live, by trying exactly that approach first and
# watching `systemctl --user show -p Result` come back empty even locally,
# not just in CI. So the actual problem is journald indexing lag (confirmed
# via a diagnostic dump on a real CI failure: the line was present moments
# later, just not within the check's own retry window at the time) --
# force a sync instead of guessing at a timeout.
journalctl --user --sync 2>/dev/null || sudo journalctl --sync 2>/dev/null || true
FAILED_OK=0
for _ in $(seq 1 20); do
    if journalctl --user -n 500 --no-pager 2>/dev/null | grep -qE "wayland-wm@.*\.service: Failed with result"; then
        FAILED_OK=1
        break
    fi
    sleep 0.3
done
if [ "$FAILED_OK" -ne 1 ]; then
    echo "---- diagnostic: unit status (may already be collected) ----" >&2
    systemctl --user status 'wayland-wm@*.service' --no-pager -l >&2 || true
    echo "---- diagnostic: all journal lines mentioning wayland-wm@ ----" >&2
    journalctl --user -n 500 --no-pager 2>/dev/null | grep -E "wayland-wm@" >&2 || true
    fail "wayland-wm@ never recorded a Failed result -- may be stuck instead of cleanly failing"
fi
echo "PASS: wayland-wm@ recorded a clean Failed result, not an indefinite hang"

echo "== asserting the session never reached full readiness (graphical-session.target) =="
[ "$(systemctl --user is-active graphical-session.target 2>&1)" != active ] \
    || fail "graphical-session.target became active despite finalize's readiness notify failing"
echo "PASS: graphical-session.target never activated"

echo "== asserting the shutdown cascade tore the whole graph down anyway =="
for _ in $(seq 1 20); do
    [ "$(systemctl --user is-active graphical-session-pre.target 2>&1)" != active ] && break
    sleep 0.3
done
[ "$(systemctl --user is-active graphical-session-pre.target 2>&1)" != active ] \
    || fail "graphical-session-pre.target is still active after the finalize failure"
for _ in $(seq 1 20); do
    pgrep -x socat >/dev/null 2>&1 || break
    sleep 0.5
done
if pgrep -x socat >/dev/null 2>&1; then
    fail "socat (wayland-stub listener) is still running after the finalize-failure shutdown cascade"
fi
echo "PASS: the shutdown cascade tore down the pre-readiness graph and the compositor's child process"

echo "== asserting the graph did not end up stuck half-configured: environment fully restored =="
systemctl --user reset-failed >/dev/null 2>&1 || true
POST_ENV="$(systemctl --user show-environment)"
[ "$PRE_ENV" = "$POST_ENV" ] \
    || fail "activation environment was not fully restored after the finalize partial failure -- exported vars from the successful half of finalize were left behind"
echo "PASS: activation environment matches the pre-session baseline -- the exported-then-orphaned vars were still cleaned up"
