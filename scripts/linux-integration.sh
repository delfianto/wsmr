#!/usr/bin/env bash
# End-to-end wsmr integration test on real systemd, in a Podman container.
#
# 1. build the Linux binary (Tier A) into the shared target volume
# 2. boot the systemd (Tier B) container as PID 1
# 3. start a user systemd manager + session bus for a test user (via linger)
# 4. run tests/integration/smoke.sh, which drives `wsmr start` with a stub
#    compositor and asserts the session lifecycle
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
IMG_BUILD="wsmr-linux-dev"
IMG_SYS="wsmr-linux-systemd"

echo "==> building the Linux binary (Tier A)"
podman build -t "$IMG_BUILD" -f "$ROOT/Containerfile" "$ROOT"
podman run --rm \
    -v "$ROOT:/workspace" \
    -v wsmr-cargo-registry:/root/.cargo/registry \
    -v wsmr-linux-target:/workspace/target \
    "$IMG_BUILD" bash -lc "cargo build"

echo "==> building the systemd image (Tier B)"
podman build -t "$IMG_SYS" -f "$ROOT/Containerfile.systemd" "$ROOT"

echo "==> booting the systemd container"
CID=$(podman run -d --systemd=always \
    -v wsmr-linux-target:/opt/wsmr-target:ro \
    -v "$ROOT/tests/integration:/opt/it:ro" \
    "$IMG_SYS")
trap 'podman rm -f "$CID" >/dev/null 2>&1 || true' EXIT

# wait for the system manager, then start the test user's manager via linger
podman exec "$CID" sh -c \
    'for i in $(seq 1 60); do systemctl is-system-running 2>/dev/null | grep -qE "running|degraded" && break; sleep 0.5; done'
podman exec "$CID" loginctl enable-linger tester >/dev/null
UID_T=$(podman exec "$CID" id -u tester)
podman exec "$CID" sh -c \
    "for i in \$(seq 1 40); do systemctl is-active user@${UID_T}.service 2>/dev/null | grep -q active && break; sleep 0.5; done"

echo "==> running the smoke test as tester (uid ${UID_T})"
# XDG_SEAT/XDG_SESSION_ID are pre-set so prepare-env skips the VT/logind
# deduction that has no meaning in a container.
podman exec -u tester \
    -e XDG_RUNTIME_DIR="/run/user/${UID_T}" \
    -e DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/${UID_T}/bus" \
    -e XDG_SEAT=seat0 -e XDG_SESSION_ID=1 -e XDG_CURRENT_DESKTOP=stub \
    "$CID" bash /opt/it/smoke.sh

echo "==> integration test PASSED"
