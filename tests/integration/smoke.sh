#!/usr/bin/env bash
# Runs *inside* the systemd container as the test user. Drives a full session
# bootstrap with the stub compositor, launches several apps, exercises
# finalize/app-daemon/duplicate-start/stop-when-stopped, then stops —
# asserting the whole lifecycle is *observably* correct rather than merely
# executed. Every claimed behavior below is a hard assertion; `set -euo
# pipefail` means an unexpected failure anywhere stops the script instead of
# silently passing (P4-01). Deliberately-expected-to-fail commands are always
# wrapped in `if ...; then fail ...; fi`, never bare `|| true`, so a genuine
# crash still surfaces.
set -euo pipefail

# Overridable so the coverage harness can point us at an instrumented binary /
# the source-tree stub (coverage-run.sh sets WSMR + STUB).
WSMR="${WSMR:-/opt/wsmr-target/debug/wsmr}"
STUB="${STUB:-/opt/it/stub-compositor.sh}"
IT_DIR="$(cd "$(dirname "$STUB")" && pwd)"
RT="/run/user/$(id -u)"

fail() { echo "FAIL: $1" >&2; exit 1; }

# Collect failed-unit state and the recent user journal on any failure exit —
# including one `set -e` triggers on an unguarded nonzero command, not just
# explicit `fail` calls (P4-01: "add a trap that collects status and
# journals on failure").
collect_diagnostics() {
    echo "---- diagnostics: failed units ----" >&2
    systemctl --user list-units --failed --no-legend >&2 || true
    echo "---- diagnostics: recent user journal (last 300 lines) ----" >&2
    journalctl --user -n 300 --no-pager >&2 || true
}
trap 'rc=$?; if [ "$rc" -ne 0 ]; then collect_diagnostics; fi' EXIT

# Under the coverage harness LLVM_PROFILE_FILE is set; propagate it into the
# user manager's activation environment so every unit-spawned wsmr process
# (prepare-env, exec, readiness, waitpid, cleanup) is instrumented too. No-op
# otherwise.
if [ -n "${LLVM_PROFILE_FILE:-}" ]; then
    systemctl --user set-environment LLVM_PROFILE_FILE="$LLVM_PROFILE_FILE" 2>/dev/null || true
fi

# Desktop-entry + fake-terminal fixtures so the `app` entry/terminal
# resolution paths are exercised end to end and their outcomes asserted (not
# just executed for coverage).
APPS="$HOME/.local/share/applications"
mkdir -p "$APPS"
cat > "$APPS/wsmrtest.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=WSMR Test App
GenericName=Tester
Exec=$IT_DIR/marker-app.sh
EOF
cat > "$APPS/wsmrterm.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=WSMR Fake Terminal
Exec=$IT_DIR/fake-terminal.sh
Categories=Utility;TerminalEmulator;
TerminalArgExec=-e
EOF
cat > "$APPS/wsmrmulti.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=WSMR Multi
Exec=$IT_DIR/multi-instance-app.sh %f
EOF

# `check may-start` BEFORE any session is active: in verbose mode it walks every
# precondition check (login-shell, VT, logind session VTNr/Remote, system
# graphical.target) instead of short-circuiting — exercising session/check.rs and
# the logind/system-bus probes in sysd/dbus.rs. Expected to refuse (exit 1,
# `check_may_start`'s documented refusal code — not just "some nonzero").
echo "== check may-start (pre-start, verbose: traverse all checks) =="
if "$WSMR" check may-start --verbose --vtnr 1 --gst-seconds 1; then
    fail "check may-start succeeded before any session context was set up"
fi
# --no-login and --vtnr 0 skip their own checks (vtnr=[0] means "no VT check"
# per session/check.rs), exercising a different branch set than the call
# above. --gst-seconds is *not* a reliable way to force a refusal here: this
# base image's compiled-in default target is graphical.target, and systemd
# considers it trivially reached with no display manager at all, so it is a
# poor proxy for "a real desktop is up" in a headless container (same as it
# would be on a bare install with no DM) — that's expected, upstream-matching
# behavior, not a bug. Omitting --allow-remote instead forces the
# session-remote lookup, which reliably refuses because XDG_SESSION_ID=1 here
# is a plain env var, not a real logind session.
if "$WSMR" check may-start --verbose --no-login --vtnr 0 --gst-seconds 1; then
    fail "check may-start (no-login/vtnr 0, gst-seconds 1) succeeded before any session context was set up"
fi
echo "PASS: check may-start refused and traversed pre-start checks"

# So stub-compositor.sh (running as the compositor unit's own ExecStart) can
# call `wsmr finalize` itself, inheriting this unit's real $NOTIFY_SOCKET —
# see stub-compositor.sh for why that's the correct way to exercise finalize.
# Set before the baseline snapshot: wsmr's own cleanup never touches this var
# (it wasn't set through wsmr's export mechanism), so it must be part of both
# the pre- and post-session baselines to compare equal.
systemctl --user set-environment WSMR_BIN="$WSMR"

echo "== capturing the pre-session activation environment baseline =="
PRE_ENV="$(systemctl --user show-environment)"

echo "== starting session =="
"$WSMR" start "$STUB" >/tmp/wsmr-start.log 2>&1 &
START_PID=$!

for _ in $(seq 1 40); do
    systemctl --user is-active graphical-session.target >/dev/null 2>&1 && break
    sleep 0.5
done

[ "$(systemctl --user is-active graphical-session.target 2>&1)" = active ] \
    || fail "graphical-session.target did not become active"
WM_UNIT=$(systemctl --user list-units --no-legend 'wayland-wm@*.service' | awk '{print $1}' | head -1)
[ -n "$WM_UNIT" ] || fail "compositor unit is not active"
[ "$(systemctl --user is-active "$WM_UNIT")" = active ] || fail "compositor unit $WM_UNIT is not active"
systemctl --user show-environment | grep -q '^WAYLAND_DISPLAY=' \
    || fail "WAYLAND_DISPLAY was not exported"
echo "PASS: session reached graphical-session.target with WAYLAND_DISPLAY, unit=$WM_UNIT"

echo "== asserting the Wayland socket is a real socket =="
[ -S "$RT/wayland-stub" ] || fail "\$XDG_RUNTIME_DIR/wayland-stub is not a socket"
echo "PASS: wayland-stub is a real Unix socket"

echo "== asserting ExecStart/FragmentPath/DropInPaths/ownership for $WM_UNIT =="
systemctl --user show -p ExecStart --value "$WM_UNIT" | grep -qF "$WSMR" \
    || fail "$WM_UNIT's ExecStart does not reference the intended wsmr binary ($WSMR)"
FRAGPATH=$(systemctl --user show -p FragmentPath --value "$WM_UNIT")
case "$FRAGPATH" in
    "$RT"/systemd/user/*) ;;
    *) fail "$WM_UNIT's FragmentPath ($FRAGPATH) is not under the runtime rung" ;;
esac
DROPINS=$(systemctl --user show -p DropInPaths --value "$WM_UNIT")
[ -n "$DROPINS" ] || fail "$WM_UNIT has no drop-ins (expected a 50_custom.conf hardcoding the stub path)"
MANIFEST="$RT/systemd/user/.wsmr-generation"
[ -f "$MANIFEST" ] || fail "ownership manifest $MANIFEST was not written"
grep -q '50_custom.conf' "$MANIFEST" || fail "manifest does not list the compositor's drop-in"
echo "PASS: ExecStart/FragmentPath/DropInPaths/manifest all check out"

echo "== asserting prepare-env completion and XDG autostart activation =="
WM_ENV_UNIT=$(systemctl --user list-units --no-legend 'wayland-wm-env@*.service' | awk '{print $1}' | head -1)
[ -n "$WM_ENV_UNIT" ] || fail "no wayland-wm-env@ unit found"
[ "$(systemctl --user is-active "$WM_ENV_UNIT")" = active ] \
    || fail "$WM_ENV_UNIT is not active (prepare-env did not complete cleanly)"
XDG_AUTOSTART_UNIT=$(systemctl --user list-units --no-legend 'wayland-session-xdg-autostart@*.target' | awk '{print $1}' | head -1)
[ -n "$XDG_AUTOSTART_UNIT" ] || fail "no wayland-session-xdg-autostart@ target found"
[ "$(systemctl --user is-active "$XDG_AUTOSTART_UNIT")" = active ] \
    || fail "$XDG_AUTOSTART_UNIT is not active"
echo "PASS: prepare-env completed ($WM_ENV_UNIT active), XDG autostart target active"

echo "== duplicate start is refused without touching the running session =="
if "$WSMR" start "$STUB" 2>/tmp/wsmr-dup-start.log; then
    fail "a second 'wsmr start' succeeded while a session was already active"
fi
grep -qi "already active" /tmp/wsmr-dup-start.log \
    || fail "duplicate-start refusal message was unexpected: $(cat /tmp/wsmr-dup-start.log)"
[ "$(systemctl --user is-active "$WM_UNIT")" = active ] \
    || fail "the original compositor unit is no longer active after the refused duplicate start"
echo "PASS: duplicate start correctly refused, original session untouched"

echo "== launching an app (service in app-graphical.slice) =="
"$WSMR" app -t service -- sleep 600 || fail "wsmr app exited non-zero"
sleep 1
APP_UNIT=$(systemctl --user list-units --no-legend 'app-*.service' 2>/dev/null | awk '{print $1}' | head -1)
[ -n "$APP_UNIT" ] || fail "no app unit was created"
[ "$(systemctl --user is-active "$APP_UNIT")" = active ] || fail "app unit $APP_UNIT not active"
[ "$(systemctl --user show -p Slice --value "$APP_UNIT")" = app-graphical.slice ] \
    || fail "app unit not in app-graphical.slice"
echo "PASS: app launched as $APP_UNIT in app-graphical.slice"

echo "== launching a desktop entry and asserting its marker/unit/slice/PID =="
rm -f /tmp/wsmr-marker-desktopapp
BEFORE_UNITS=$(systemctl --user list-units --no-legend 'app-*.service' 2>/dev/null | awk '{print $1}' | sort)
"$WSMR" app -t service -- wsmrtest.desktop || fail "wsmr app (desktop entry) exited non-zero"
for _ in $(seq 1 20); do [ -f /tmp/wsmr-marker-desktopapp ] && break; sleep 0.2; done
[ -f /tmp/wsmr-marker-desktopapp ] || fail "desktop-entry app never wrote its marker file"
sleep 1
AFTER_UNITS=$(systemctl --user list-units --no-legend 'app-*.service' 2>/dev/null | awk '{print $1}' | sort)
DESKTOP_APP_UNIT=$(comm -13 <(echo "$BEFORE_UNITS") <(echo "$AFTER_UNITS") | head -1)
[ -n "$DESKTOP_APP_UNIT" ] || fail "no new app unit appeared for the desktop-entry launch"
[ "$(systemctl --user is-active "$DESKTOP_APP_UNIT")" = active ] \
    || fail "desktop-entry app unit $DESKTOP_APP_UNIT is not active"
[ "$(systemctl --user show -p Slice --value "$DESKTOP_APP_UNIT")" = app-graphical.slice ] \
    || fail "desktop-entry app unit not in app-graphical.slice"
APP_PID=$(systemctl --user show -p MainPID --value "$DESKTOP_APP_UNIT")
[ -n "$APP_PID" ] && [ "$APP_PID" -gt 0 ] && [ -d "/proc/$APP_PID" ] \
    || fail "desktop-entry app unit $DESKTOP_APP_UNIT has no valid running MainPID"
echo "PASS: desktop entry launched as $DESKTOP_APP_UNIT (PID $APP_PID), marker written, correct slice"

echo "== app: terminal resolution (real fake-terminal, hard assertion) =="
rm -f /tmp/wsmr-fake-term.log
"$WSMR" app -T -- true || fail "wsmr app -T (terminal launch) exited non-zero"
for _ in $(seq 1 20); do [ -s /tmp/wsmr-fake-term.log ] && break; sleep 0.2; done
[ -s /tmp/wsmr-fake-term.log ] || fail "fake terminal was never invoked"
grep -q -- '-e' /tmp/wsmr-fake-term.log \
    || fail "fake terminal was not invoked with -e: $(cat /tmp/wsmr-fake-term.log)"
echo "PASS: terminal launched via fake-terminal.sh (logged: $(cat /tmp/wsmr-fake-term.log))"

echo "== app: multi-instance resolution ('%f' fan-out to two units) =="
BEFORE_UNITS=$(systemctl --user list-units --no-legend 'app-*.service' 2>/dev/null | awk '{print $1}' | sort)
"$WSMR" app -t service -- wsmrmulti.desktop /etc/hostname /etc/hosts \
    || fail "wsmr app (multi-instance) exited non-zero"
sleep 1
AFTER_UNITS=$(systemctl --user list-units --no-legend 'app-*.service' 2>/dev/null | awk '{print $1}' | sort)
NEW_COUNT=$(comm -13 <(echo "$BEFORE_UNITS") <(echo "$AFTER_UNITS") | grep -c . || true)
[ "$NEW_COUNT" -ge 2 ] || fail "multi-instance launch created $NEW_COUNT unit(s), expected >= 2"
echo "PASS: multi-instance launch created $NEW_COUNT units"

echo "== finalize: verifying its variable-export effect =="
# finalize() already ran for real as part of session start: stub-compositor.sh
# calls it directly (inheriting the compositor unit's own $NOTIFY_SOCKET, the
# way a real self-integrating compositor does — see stub-compositor.sh). A
# failure there would have kept the unit from ever reaching readiness, which
# the graphical-session.target assertion above already caught with a hard
# timeout; this checks its other job — exporting extra_vars — actually
# happened, not just "the process didn't crash".
systemctl --user show-environment | grep -q '^XDG_CURRENT_DESKTOP=stub$' \
    || fail "finalize did not export XDG_CURRENT_DESKTOP into the activation environment"
echo "PASS: finalize exported XDG_CURRENT_DESKTOP (ran inside the compositor's own notify context)"

echo "== app-daemon (FIFO ping/pong + app resolution) =="
"$WSMR" aux app-daemon >/tmp/wsmr-daemon.log 2>&1 &
DPID=$!
for _ in $(seq 1 20); do [ -p "$RT/wsmr-app-daemon-in" ] && break; sleep 0.2; done
[ -p "$RT/wsmr-app-daemon-in" ] || fail "app-daemon did not create its in-FIFO"
# NUL-separated argv via `printf '%s\0'` (a bare \0NNN would be misread as octal)
printf '%s\0' ping > "$RT/wsmr-app-daemon-in"
PONG=$(timeout 10 head -1 "$RT/wsmr-app-daemon-out")
[ "$PONG" = pong ] || fail "app-daemon ping returned: '$PONG'"
printf '%s\0' app -- sleep 600 > "$RT/wsmr-app-daemon-in"
RESP=$(timeout 10 head -1 "$RT/wsmr-app-daemon-out")
case "$RESP" in
    "exec systemd-run --user --scope"*) ;;
    *) fail "app-daemon emitted unexpected line: '$RESP'" ;;
esac

echo "== app-daemon: missing reader on a reply is bounded, not fatal (P4-03) =="
printf '%s\0' ping > "$RT/wsmr-app-daemon-in"
# Deliberately don't read wsmr-app-daemon-out this time: the daemon's bounded
# FIFO-open (P5-04) must give up after its 5s SEND_TIMEOUT and log, not hang
# the daemon loop or crash it.
sleep 7
printf '%s\0' ping > "$RT/wsmr-app-daemon-in"
PONG2=$(timeout 10 head -1 "$RT/wsmr-app-daemon-out")
[ "$PONG2" = pong ] || fail "app-daemon did not recover after an unread reply (missing-reader scenario)"
echo "PASS: app-daemon survived a missing-reader reply and served the next request"

# Clean shutdown: `stop` no longer writes to the out-FIFO (so it can't block on a
# missing reader) — the daemon removes its FIFOs and exits 0.
printf '%s\0' stop > "$RT/wsmr-app-daemon-in"
wait "$DPID" 2>/dev/null || true
[ ! -p "$RT/wsmr-app-daemon-in" ] || fail "app-daemon did not remove its in-FIFO on stop"
echo "PASS: app-daemon answered ping, resolved an app command, and stopped cleanly"

echo "== check may-start (should refuse: session active) =="
if "$WSMR" check may-start --no-login --vtnr 0 --gst-seconds 0 -q; then
    fail "check may-start succeeded while a session is active"
fi
echo "PASS: check may-start refused (session already active)"

echo "== stopping session via wsmr stop =="
"$WSMR" stop || fail "wsmr stop exited non-zero"
wait "$START_PID" 2>/dev/null || true
sleep 1

[ "$(systemctl --user is-active graphical-session.target 2>&1)" != active ] \
    || fail "graphical-session.target still active after stop"
systemctl --user show-environment | grep -q '^WAYLAND_DISPLAY=' \
    && fail "WAYLAND_DISPLAY was not cleaned up"
echo "PASS: wsmr stop tore down the session"

echo "== stop when already stopped is a clean no-op =="
"$WSMR" stop || fail "'wsmr stop' exited non-zero when nothing was running"
echo "PASS: stop-when-already-stopped is a clean no-op"

echo "== asserting the compositor and its child processes are gone =="
if pgrep -f stub-compositor.sh >/dev/null 2>&1; then
    fail "stub-compositor.sh process is still running after stop"
fi
if pgrep -x socat >/dev/null 2>&1; then
    fail "socat (wayland-stub listener) is still running after stop"
fi
echo "PASS: compositor and its child processes are gone"

echo "== comparing the restored environment against the pre-session baseline =="
POST_ENV="$(systemctl --user show-environment)"
if [ "$PRE_ENV" != "$POST_ENV" ]; then
    {
        echo "pre-session environment:"
        echo "$PRE_ENV"
        echo "post-session environment:"
        echo "$POST_ENV"
    } >&2
    fail "systemd activation environment was not fully restored after stop"
fi
echo "PASS: activation environment matches the pre-session baseline exactly"

echo "== asserting no failed units and no stale wsmr runtime state =="
FAILED="$(systemctl --user list-units --failed --no-legend)"
[ -z "$FAILED" ] || fail "failed units present after the full lifecycle: $FAILED"
if [ -d "$RT/wsmr" ]; then
    # state.lock is intentionally permanent (session/state.rs): it's an flock
    # target, deliberately never deleted so a lock is never divorced from its
    # path out from under a concurrent holder. Everything else under
    # $XDG_RUNTIME_DIR/wsmr is per-session and must be gone.
    REMAINING="$(find "$RT/wsmr" -type f -not -name 'state.lock')"
    [ -z "$REMAINING" ] || fail "stale files remain under \$XDG_RUNTIME_DIR/wsmr: $REMAINING"
fi
if find "$RT/systemd/user" -maxdepth 2 -name '.*.wsmr-tmp.*' 2>/dev/null | grep -q .; then
    fail "leftover .wsmr-tmp temp files in the unit directory"
fi
echo "PASS: no failed units, no stale wsmr runtime state"
