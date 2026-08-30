#!/bin/sh
# Deliberately-broken stub compositor for the "readiness timeout" P4-03
# scenario: creates a real Wayland socket (so the process itself looks
# alive and running) but never exports WAYLAND_DISPLAY or calls `wsmr
# finalize` — simulating a compositor that starts but never signals
# readiness. Paired with a short $UWSM_WAIT_VARNAMES_TIMEOUT so the
# `wayland-session-waitenv.service` timeout fires quickly instead of after
# the real 30s default.
set -eu

SOCK_NAME=wayland-stub
SOCK_PATH="${XDG_RUNTIME_DIR:?}/$SOCK_NAME"
rm -f "$SOCK_PATH"

socat UNIX-LISTEN:"$SOCK_PATH",fork /dev/null &

for _ in $(seq 1 50); do
    [ -S "$SOCK_PATH" ] && break
    sleep 0.1
done
[ -S "$SOCK_PATH" ] || {
    echo "stub-compositor-hang: $SOCK_PATH never became a socket" >&2
    exit 1
}

echo "stub-compositor-hang: socket is up, deliberately never signaling readiness" >&2
exec sleep infinity
