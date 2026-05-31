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
