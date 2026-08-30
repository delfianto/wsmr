#!/bin/sh
# Deliberately-broken stub compositor for the "finalize partial failure" P4-03
# scenario: opens a real socket and calls `wsmr finalize` itself (inheriting
# this unit's own $NOTIFY_SOCKET, same as the real stub-compositor.sh), but
# with `systemd-notify` shadowed by a stand-in that always fails. finalize's
# env-export half (WAYLAND_DISPLAY/DISPLAY into the activation environment)
# still runs and succeeds -- only the readiness-notify half fails, so this
# compositor's own process exits nonzero right after exec-ing into the fake
# notify, and wayland-wm@'s Type=notify unit sees a dead main process instead
# of a READY=1 it was waiting for.
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
    echo "stub-compositor-badnotify: $SOCK_PATH never became a socket" >&2
    exit 1
}

export WAYLAND_DISPLAY="$SOCK_NAME"
export DISPLAY=":0"

FAKE_NOTIFY_DIR="${XDG_RUNTIME_DIR}/fake-notify-bin"
mkdir -p "$FAKE_NOTIFY_DIR"
cat > "$FAKE_NOTIFY_DIR/systemd-notify" <<'EOF'
#!/bin/sh
echo "stub-compositor-badnotify: fake systemd-notify deliberately failing, args: $*" >&2
exit 1
EOF
chmod +x "$FAKE_NOTIFY_DIR/systemd-notify"
export PATH="$FAKE_NOTIFY_DIR:$PATH"

echo "stub-compositor-badnotify: socket is up, calling wsmr finalize with a broken systemd-notify" >&2
exec "${WSMR_BIN:?}" finalize
