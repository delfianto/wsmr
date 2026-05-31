#!/usr/bin/env bash
# Runs *inside* the systemd container as the test user. Drives a full session
# bootstrap with the stub compositor and asserts the lifecycle.
set -uo pipefail

WSMR=/opt/wsmr-target/debug/wsmr
STUB=/opt/it/stub-compositor.sh

fail() { echo "FAIL: $1" >&2; exit 1; }

echo "== starting session =="
"$WSMR" start "$STUB" >/tmp/wsmr-start.log 2>&1 &
START_PID=$!

# wait for the session to come up
for _ in $(seq 1 40); do
    systemctl --user is-active graphical-session.target >/dev/null 2>&1 && break
    sleep 0.5
done

[ "$(systemctl --user is-active graphical-session.target 2>&1)" = active ] \
    || fail "graphical-session.target did not become active"
systemctl --user list-units --no-legend 'wayland-wm@*.service' | grep -q ' active ' \
    || fail "compositor unit is not active"
systemctl --user show-environment | grep -q '^WAYLAND_DISPLAY=' \
    || fail "WAYLAND_DISPLAY was not exported to the activation environment"
echo "PASS: session reached graphical-session.target with WAYLAND_DISPLAY"

echo "== stopping session =="
systemctl --user start wayland-session-shutdown.target 2>/dev/null || true
wait "$START_PID" 2>/dev/null || true
sleep 1

[ "$(systemctl --user is-active graphical-session.target 2>&1)" != active ] \
    || fail "graphical-session.target still active after shutdown"
systemctl --user show-environment | grep -q '^WAYLAND_DISPLAY=' \
    && fail "WAYLAND_DISPLAY was not cleaned up"
[ ! -f /run/user/"$(id -u)"/wsmr/env_cleanup.list ] \
    || fail "env_cleanup.list was not removed on cleanup"
echo "PASS: shutdown tore down the session and cleaned the environment"
