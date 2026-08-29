#!/bin/sh
# Stub Wayland compositor for wsmr integration tests.
#
# A real compositor creates a Wayland socket and puts WAYLAND_DISPLAY into
# the systemd/D-Bus activation environment. This stub does both: a real
# listening Unix socket at $XDG_RUNTIME_DIR/wayland-stub (so "the socket
# exists and is a socket" is a real assertion, not just an env var), then
# calls `wsmr finalize` itself — the way a real self-integrating compositor
# (Sway, Hyprland, ...) does — rather than relying only on wsmr's fallback
# `aux readiness` watcher. Run as this unit's own ExecStart, it's a
# foreground child of the same invocation and so inherits the *real*
# $NOTIFY_SOCKET systemd provisioned for this unit — proving finalize works
# in the context it's actually designed for, not a detached unit that never
# had a notify socket to begin with. WSMR_BIN is pushed into the manager's
# activation environment by smoke.sh before `wsmr start` runs.
set -eu

SOCK_NAME=wayland-stub
SOCK_PATH="${XDG_RUNTIME_DIR:?}/$SOCK_NAME"
rm -f "$SOCK_PATH"

# `socat UNIX-LISTEN:path,fork /dev/null` binds and listens on a real Unix
# socket, discarding whatever connects — enough to make the socket file real
# without speaking any Wayland protocol (wsmr never does either).
socat UNIX-LISTEN:"$SOCK_PATH",fork /dev/null &

# No cleanup trap for socat: it's about to become our sibling in the same
# cgroup once `exec sleep infinity` below replaces this shell (a trap set
# now wouldn't survive that exec anyway) — systemd's default
# KillMode=control-group takes the whole cgroup down together when the unit
# stops, socat included.
for _ in $(seq 1 50); do
    [ -S "$SOCK_PATH" ] && break
    sleep 0.1
done
[ -S "$SOCK_PATH" ] || {
    echo "stub-compositor: $SOCK_PATH never became a socket" >&2
    exit 1
}

export WAYLAND_DISPLAY="$SOCK_NAME"
"${WSMR_BIN:?}" finalize XDG_CURRENT_DESKTOP

exec sleep infinity
