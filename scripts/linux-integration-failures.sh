#!/usr/bin/env bash
# P4-03 deferred failure/recovery scenarios, each on its own fresh systemd
# (Tier B) container boot — see docs/fix-plan.md's scope note on why each
# needs isolation: a broken scenario can corrupt systemd state a later
# scenario in the same run would depend on.
#
# Usage: scripts/linux-integration-failures.sh [scenario...]
# With no arguments, runs all scenarios. Scenario names: crash-before-readiness,
# readiness-timeout, unclean-exit.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
IMG_BUILD="wsmr-linux-dev"
IMG_SYS="wsmr-linux-systemd"

ALL_SCENARIOS=(crash-before-readiness readiness-timeout unclean-exit)
SCENARIOS=("${@:-${ALL_SCENARIOS[@]}}")

echo "==> building the Linux binary (Tier A)"
podman build -t "$IMG_BUILD" -f "$ROOT/Containerfile" "$ROOT"
podman run --rm \
    -v "$ROOT:/workspace" \
    -v wsmr-cargo-registry:/root/.cargo/registry \
    -v wsmr-linux-target:/workspace/target \
    "$IMG_BUILD" bash -lc "cargo build"

echo "==> building the systemd image (Tier B)"
podman build -t "$IMG_SYS" -f "$ROOT/Containerfile.systemd" "$ROOT"

run_scenario() {
    local name="$1" script="$2"
    echo "==================================================================="
    echo "==> scenario: $name (fresh container boot)"
    echo "==================================================================="
    local cid
    cid=$(podman run -d --systemd=always \
        -v wsmr-linux-target:/opt/wsmr-target:ro \
        -v "$ROOT/tests/integration:/opt/it:ro" \
        "$IMG_SYS")
    # shellcheck disable=SC2064
    trap "podman rm -f '$cid' >/dev/null 2>&1 || true" RETURN

    podman exec "$cid" sh -c \
        'for i in $(seq 1 60); do systemctl is-system-running 2>/dev/null | grep -qE "running|degraded" && break; sleep 0.5; done'
    podman exec "$cid" loginctl enable-linger tester >/dev/null
    local uid_t
    uid_t=$(podman exec "$cid" id -u tester)
    podman exec "$cid" sh -c \
        "for i in \$(seq 1 40); do systemctl is-active user@${uid_t}.service 2>/dev/null | grep -q active && break; sleep 0.5; done"

    if podman exec -u tester \
        -e XDG_RUNTIME_DIR="/run/user/${uid_t}" \
        -e DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/${uid_t}/bus" \
        -e XDG_SEAT=seat0 -e XDG_SESSION_ID=1 -e XDG_CURRENT_DESKTOP=stub \
        "$cid" bash "/opt/it/$script"; then
        echo "==> scenario $name: PASSED"
    else
        echo "==> scenario $name: FAILED" >&2
        podman rm -f "$cid" >/dev/null 2>&1 || true
        trap - RETURN
        return 1
    fi
    podman rm -f "$cid" >/dev/null 2>&1 || true
    trap - RETURN
}

FAILED_SCENARIOS=()
for s in "${SCENARIOS[@]}"; do
    case "$s" in
        crash-before-readiness) run_scenario "$s" smoke-crash-before-readiness.sh || FAILED_SCENARIOS+=("$s") ;;
        readiness-timeout) run_scenario "$s" smoke-readiness-timeout.sh || FAILED_SCENARIOS+=("$s") ;;
        unclean-exit) run_scenario "$s" smoke-unclean-exit.sh || FAILED_SCENARIOS+=("$s") ;;
        *) echo "unknown scenario: $s" >&2; exit 2 ;;
    esac
done

if [ "${#FAILED_SCENARIOS[@]}" -gt 0 ]; then
    echo "==> FAILED scenarios: ${FAILED_SCENARIOS[*]}" >&2
    exit 1
fi
echo "==> all requested failure/recovery scenarios PASSED"
