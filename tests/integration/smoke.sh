#!/usr/bin/env bash
# Runs *inside* the systemd container as the test user. Drives a full session
# bootstrap with the stub compositor, launches an app, then stops — asserting
# the whole lifecycle.
set -uo pipefail

WSMR=/opt/wsmr-target/debug/wsmr
STUB=/opt/it/stub-compositor.sh

fail() { echo "FAIL: $1" >&2; exit 1; }

echo "== starting session =="
"$WSMR" start "$STUB" >/tmp/wsmr-start.log 2>&1 &
START_PID=$!

for _ in $(seq 1 40); do
    systemctl --user is-active graphical-session.target >/dev/null 2>&1 && break
    sleep 0.5
done

[ "$(systemctl --user is-active graphical-session.target 2>&1)" = active ] \
    || fail "graphical-session.target did not become active"
systemctl --user list-units --no-legend 'wayland-wm@*.service' | grep -q ' active ' \
    || fail "compositor unit is not active"
systemctl --user show-environment | grep -q '^WAYLAND_DISPLAY=' \
    || fail "WAYLAND_DISPLAY was not exported"
echo "PASS: session reached graphical-session.target with WAYLAND_DISPLAY"

echo "== launching an app (service in app-graphical.slice) =="
"$WSMR" app -t service -- sleep 600 || fail "wsmr app exited non-zero"
sleep 1
APP_UNIT=$(systemctl --user list-units --no-legend 'app-*.service' 2>/dev/null | awk '{print $1}' | head -1)
[ -n "$APP_UNIT" ] || fail "no app unit was created"
[ "$(systemctl --user is-active "$APP_UNIT")" = active ] || fail "app unit $APP_UNIT not active"
[ "$(systemctl --user show -p Slice --value "$APP_UNIT")" = app-graphical.slice ] \
    || fail "app unit not in app-graphical.slice"
echo "PASS: app launched as $APP_UNIT in app-graphical.slice"

echo "== app-daemon (FIFO ping/pong + app resolution) =="
RT="/run/user/$(id -u)"
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
# Terminate the daemon directly (sending `stop` would have it reply to the
# out-FIFO, which blocks for a reader we don't provide here).
kill "$DPID" 2>/dev/null || true
wait "$DPID" 2>/dev/null || true
echo "PASS: app-daemon answered ping and resolved an app command"

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
[ ! -f /run/user/"$(id -u)"/wsmr/env_cleanup.list ] \
    || fail "env_cleanup.list was not removed on cleanup"
echo "PASS: wsmr stop tore down the session and cleaned the environment"
