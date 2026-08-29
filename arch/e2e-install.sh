#!/usr/bin/env bash
# Build wsmr and install it at a stable, versioned E2E test path, plus the
# matching wayland-sessions entry. Run as root (needs to write under
# /usr/local and /usr/share) from anywhere; paths are resolved relative to
# this script, not $PWD.
#
# What it does:
#   1. cargo build --release --locked, from the crate root (not arch/PKGBUILD
#      — no need to register a package in pacman's DB just to test a binary)
#   2. install the binary at /usr/local/libexec/wsmr-e2e/<version>/wsmr
#   3. generate /usr/share/wayland-sessions/wsmr-e2e.desktop from
#      session/wsmr-e2e.desktop.in, pointing at that exact path
#
# Rerunning after a code change rebuilds and reinstalls at the (possibly new)
# version path and regenerates the session entry — safe to run repeatedly.
#
# What it deliberately does NOT do — the rest of arch/README.md's setup, on
# purpose: create the disposable test user, write that user's Hyprland
# config, touch /usr/bin/wsmr or the primary user's account, or run any part
# of the actual verification harness (see docs/fix-plan.md's Phase 7 for
# what that still needs).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="$(awk -F'"' '/^version/{print $2; exit}' "$ROOT/Cargo.toml")"
DEST_DIR="/usr/local/libexec/wsmr-e2e/$VERSION"
SESSION_DIR="/usr/share/wayland-sessions"
TEMPLATE="$ROOT/arch/session/wsmr-e2e.desktop.in"

if [ "$(id -u)" -ne 0 ]; then
    echo "e2e-install.sh: must run as root (installs under /usr/local and /usr/share)" >&2
    exit 1
fi

[ -f "$TEMPLATE" ] || { echo "e2e-install.sh: missing $TEMPLATE" >&2; exit 1; }

echo "==> building wsmr $VERSION (release, --locked)"
( cd "$ROOT" && cargo build --release --locked )

BIN="$ROOT/target/release/wsmr"
[ -x "$BIN" ] || { echo "e2e-install.sh: $BIN missing after build" >&2; exit 1; }

echo "==> installing to $DEST_DIR/wsmr"
install -Dm0755 "$BIN" "$DEST_DIR/wsmr"

echo "==> writing $SESSION_DIR/wsmr-e2e.desktop"
sed -e "s|@WSMR_BIN@|$DEST_DIR/wsmr|g" "$TEMPLATE" > "$SESSION_DIR/wsmr-e2e.desktop"

cat <<EOF
==> done.
    Installed binary: $DEST_DIR/wsmr
    Session entry:    $SESSION_DIR/wsmr-e2e.desktop
    Next: arch/README.md's disposable-user and Hyprland-config steps,
    then docs/fix-plan.md's Phase 7 checklist.
EOF
