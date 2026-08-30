#!/usr/bin/env bash
# Real-hardware harness for the disposable-user, real-compositor session
# (see arch/README.md for setup), verified the same way every other tier in
# this repo is - real scripted assertions with a real exit
# code, not a transcript of commands someone ran by hand once.
#
# Three stages, matching P7-02 exactly, run as root from a login shell that
# is NOT the disposable test account (reaches into that account's systemd
# --user manager/D-Bus bus the same way `sudo -u <user> env ...` does
# throughout this file, so nothing here needs a terminal open inside the
# graphical session itself - useful since a broken input stack, like the
# real kmscon/Hyprland seat conflict this harness's own design was informed
# by, can make that impossible anyway):
#
#   prepare      run BEFORE the disposable user logs in. Sanity-checks the
#                account, snapshots the pre-login activation environment and
#                package/kernel versions to a state dir, and tells you what
#                to do next.
#   verify       run WHILE a wsmr-managed session is live (after you've
#                logged in as the disposable user and run
#                `wsmr start ...`). Runs the P7-03 checklist as real,
#                independent assertions and prints a pass/fail report.
#   post-logout  run AFTER the session has ended (`wsmr stop`, or a normal
#                in-session logout). Diffs the current environment against
#                prepare's saved baseline and checks for failed units/stale
#                state.
#
# Usage:
#   scripts/e2e-harness.sh prepare     [--user wsmr]
#   scripts/e2e-harness.sh verify      [--user wsmr]
#   scripts/e2e-harness.sh post-logout [--user wsmr]
#
# Every stage is safe to rerun (prepare/post-logout overwrite their own state
# files; verify has no side effects beyond its own test app, which it cleans
# up itself). Everything is scoped to the named disposable account only -
# never touches the primary user's session, /usr/bin/wsmr, or the system
# beyond querying it.
set -uo pipefail

E2E_USER="wsmr"
STAGE="${1:-}"
shift || true
while [ "$#" -gt 0 ]; do
    case "$1" in
        --user) E2E_USER="$2"; shift 2 ;;
        *) echo "e2e-harness.sh: unknown argument '$1'" >&2; exit 2 ;;
    esac
done

case "$STAGE" in
    prepare|verify|post-logout) ;;
    *)
        echo "usage: $0 {prepare|verify|post-logout} [--user NAME]" >&2
        exit 2
        ;;
esac

if [ "$(id -u)" -ne 0 ]; then
    echo "e2e-harness.sh: must run as root (reaches into $E2E_USER's own" >&2
    echo "systemd --user manager/D-Bus bus via sudo -u, from outside it)" >&2
    exit 1
fi

if ! id "$E2E_USER" >/dev/null 2>&1; then
    echo "e2e-harness.sh: user '$E2E_USER' does not exist" >&2
    exit 1
fi

RUNTIME_UID=$(id -u "$E2E_USER")
RUNTIME_DIR="/run/user/$RUNTIME_UID"
STATE_DIR="/tmp/wsmr-e2e-harness/$E2E_USER"
mkdir -p "$STATE_DIR"

# Run a command as the disposable user, reaching its systemd --user manager
# and session D-Bus bus the same way every manual check this harness
# replaces did all session (`man sudo` env-passing rules block plain
# `sudo VAR=val cmd`, hence the explicit `env`).
u() {
    sudo -u "$E2E_USER" env \
        XDG_RUNTIME_DIR="$RUNTIME_DIR" \
        DBUS_SESSION_BUS_ADDRESS="unix:path=$RUNTIME_DIR/bus" \
        "$@"
}

# Same, but also targeting a specific live Hyprland instance for `hyprctl`
# (which additionally needs HYPRLAND_INSTANCE_SIGNATURE - hyprctl doesn't
# discover it from the session bus the way systemctl --user does).
hypr() {
    sudo -u "$E2E_USER" env \
        XDG_RUNTIME_DIR="$RUNTIME_DIR" \
        HYPRLAND_INSTANCE_SIGNATURE="$INSTANCE" \
        hyprctl "$@"
}

# Find the live wayland-wm@*.service unit for $E2E_USER, deriving the
# compositor id from it rather than assuming one - `wsmr start` can be
# invoked with any compositor argument, and today's own real runs used both
# "hyprland" and "hyprland.desktop" across otherwise-identical sessions.
discover_unit() {
    u systemctl --user list-units --no-legend 'wayland-wm@*.service' 2>/dev/null \
        | awk '{print $1}' | head -1
}

#######################################
# prepare
#######################################
run_prepare() {
    echo "== prepare: sanity checks =="

    if [ "$(u systemctl --user is-active graphical-session.target 2>/dev/null || true)" = active ]; then
        echo "e2e-harness.sh: a wsmr-managed session is already active for" >&2
        echo "'$E2E_USER' - stop it first (wsmr stop, or log out) so prepare" >&2
        echo "captures a real pre-login baseline, not a mid-session one." >&2
        exit 1
    fi
    echo "PASS: no session currently active for $E2E_USER"

    LINGER=$(loginctl show-user "$E2E_USER" -p Linger --value 2>/dev/null || echo no)
    if [ "$LINGER" != yes ]; then
        echo "WARN: linger is not enabled for $E2E_USER (loginctl enable-linger $E2E_USER)" >&2
        echo "      - systemd --user may not be running yet, so this and later" >&2
        echo "      stages could fail to reach its bus until the first login." >&2
    else
        echo "PASS: linger enabled for $E2E_USER"
    fi

    SESSION_ENTRY_COUNT=$(find /usr/share/wayland-sessions -iname '*wsmr*' 2>/dev/null | wc -l)
    if [ "$SESSION_ENTRY_COUNT" -eq 0 ]; then
        echo "WARN: no wsmr wayland-sessions entry found under" >&2
        echo "      /usr/share/wayland-sessions/ (arch/PKGBUILD or" >&2
        echo "      arch/e2e-install.sh installs one) - you can still start" >&2
        echo "      manually with 'wsmr start ...'." >&2
    else
        echo "PASS: found $SESSION_ENTRY_COUNT wsmr session entr(y/ies) under /usr/share/wayland-sessions/"
    fi

    echo "== prepare: snapshotting pre-login baseline =="
    u systemctl --user show-environment > "$STATE_DIR/pre-login-environment.txt" 2>&1
    echo "PASS: saved pre-login environment to $STATE_DIR/pre-login-environment.txt"

    {
        pacman -Q systemd dbus hyprland wsmr 2>/dev/null
        echo "kernel $(uname -r)"
        echo "captured_at $(date -Is)"
    } > "$STATE_DIR/versions.txt"
    echo "PASS: saved versions to $STATE_DIR/versions.txt:"
    sed 's/^/  /' "$STATE_DIR/versions.txt"

    cat <<EOF

==> prepare done. Next:
    1. Log in as '$E2E_USER' (display manager, or a raw VT + manual login).
    2. Run: wsmr start -e -D Hyprland hyprland.desktop
       (or whatever compositor argument you're testing - the wayland-sessions
       entry does this for you if you log in through a display manager)
    3. From here (or any other root shell), run:
         $0 verify --user $E2E_USER
EOF
}

#######################################
# verify
#######################################
CHECKS_TOTAL=0
CHECKS_FAILED=0

# Soft-fail assertion helper: verify's whole point is a complete report, so
# one failed check must not stop the rest from running (unlike smoke.sh's
# fail-fast style, which is deliberately different for a Tier-B *regression*
# gate vs. this real-hardware *survey*).
check() {
    local desc="$1"
    CHECKS_TOTAL=$((CHECKS_TOTAL + 1))
    if eval "${2}"; then
        echo "PASS: $desc"
    else
        echo "FAIL: $desc"
        CHECKS_FAILED=$((CHECKS_FAILED + 1))
    fi
}

run_verify() {
    echo "== verify: locating the live session =="
    UNIT=$(discover_unit)
    if [ -z "$UNIT" ]; then
        echo "e2e-harness.sh: no active wayland-wm@*.service for '$E2E_USER'." >&2
        echo "Log in and run 'wsmr start ...' first, then rerun verify." >&2
        exit 1
    fi
    COMPOSITOR_ID=$(echo "$UNIT" | sed -e 's/^wayland-wm@//' -e 's/\.service$//')
    INSTANCE=$(sudo find "$RUNTIME_DIR/hypr" -maxdepth 1 -mindepth 1 -type d 2>/dev/null \
        | xargs -n1 basename 2>/dev/null | sort | tail -1)
    echo "unit: $UNIT   compositor id: $COMPOSITOR_ID   hyprland instance: ${INSTANCE:-<none found>}"
    echo

    echo "== verify: P7-03 checklist =="

    check "WAYLAND_DISPLAY names a real socket" \
        "WD=\$(u systemctl --user show-environment | sed -n 's/^WAYLAND_DISPLAY=//p'); [ -n \"\$WD\" ] && sudo test -S \"$RUNTIME_DIR/\$WD\""

    if [ -n "$INSTANCE" ]; then
        check "hyprctl monitors succeeds and reports at least one real monitor" \
            "hypr monitors 2>/dev/null | grep -q '^Monitor '"
    else
        echo "SKIP: hyprctl monitors (no Hyprland instance directory found under $RUNTIME_DIR/hypr/)"
    fi

    check "compositor unit is active" \
        "[ \"\$(u systemctl --user is-active '$UNIT')\" = active ]"

    MAINPID=$(u systemctl --user show -p MainPID --value "$UNIT" 2>/dev/null)
    check "compositor PID's cgroup belongs to $UNIT" \
        "[ -n \"$MAINPID\" ] && sudo grep -q \"$UNIT\$\" /proc/$MAINPID/cgroup 2>/dev/null"

    check "FragmentPath is under the runtime rung" \
        "case \"\$(u systemctl --user show -p FragmentPath --value '$UNIT')\" in $RUNTIME_DIR/systemd/user/*) true;; *) false;; esac"

    check "unit has an ownership drop-in (DropInPaths non-empty)" \
        "[ -n \"\$(u systemctl --user show -p DropInPaths --value '$UNIT')\" ]"

    check "unit Result is success" \
        "[ \"\$(u systemctl --user show -p Result --value '$UNIT')\" = success ]"

    check "graphical-session.target is active" \
        "[ \"\$(u systemctl --user is-active graphical-session.target)\" = active ]"

    check "wayland-session@$COMPOSITOR_ID.target is active" \
        "[ \"\$(u systemctl --user is-active 'wayland-session@$COMPOSITOR_ID.target' 2>/dev/null)\" = active ]"

    check "wayland-session-xdg-autostart@$COMPOSITOR_ID.target is active" \
        "[ \"\$(u systemctl --user is-active 'wayland-session-xdg-autostart@$COMPOSITOR_ID.target' 2>/dev/null)\" = active ]"

    check "WAYLAND_DISPLAY reached the systemd manager environment" \
        "u systemctl --user show-environment | grep -q '^WAYLAND_DISPLAY='"

    check "XDG_CURRENT_DESKTOP reached the systemd manager environment" \
        "u systemctl --user show-environment | grep -q '^XDG_CURRENT_DESKTOP='"

    echo "== verify: wsmr app launches a fixture in the right unit/slice =="
    BEFORE=$(u systemctl --user list-units --no-legend 'app-*.service' 2>/dev/null | awk '{print $1}' | sort)
    u wsmr app -t service -- sleep 60 >/dev/null 2>&1
    sleep 1
    AFTER=$(u systemctl --user list-units --no-legend 'app-*.service' 2>/dev/null | awk '{print $1}' | sort)
    APP_UNIT=$(comm -13 <(echo "$BEFORE") <(echo "$AFTER") | head -1)
    check "wsmr app created a new app-*.service unit" \
        "[ -n \"$APP_UNIT\" ]"
    if [ -n "$APP_UNIT" ]; then
        check "$APP_UNIT is active" \
            "[ \"\$(u systemctl --user is-active '$APP_UNIT')\" = active ]"
        check "$APP_UNIT is in app-graphical.slice" \
            "[ \"\$(u systemctl --user show -p Slice --value '$APP_UNIT')\" = app-graphical.slice ]"
        u systemctl --user stop "$APP_UNIT" >/dev/null 2>&1 || true
    fi

    check "at least one XDG autostart app is active" \
        "[ -n \"\$(u systemctl --user list-units --no-legend 'app-*@autostart.service' 2>/dev/null)\" ]"

    echo
    echo "== verify: summary =="
    echo "$((CHECKS_TOTAL - CHECKS_FAILED))/$CHECKS_TOTAL checks passed"
    if [ "$CHECKS_FAILED" -gt 0 ]; then
        echo
        echo "== diagnostics: failed units =="
        u systemctl --user list-units --failed --no-legend 2>&1
        echo "== diagnostics: recent journal for $E2E_USER =="
        sudo journalctl "_UID=$RUNTIME_UID" -n 100 --no-pager -o short-precise 2>&1
        exit 1
    fi
}

#######################################
# post-logout
#######################################
run_post_logout() {
    echo "== post-logout: session-ended checks =="

    check "graphical-session.target is inactive" \
        "[ \"\$(u systemctl --user is-active graphical-session.target 2>&1)\" != active ]"

    check "no failed units remain" \
        "[ -z \"\$(u systemctl --user list-units --failed --no-legend 2>/dev/null)\" ]"

    echo
    echo "== post-logout: environment restoration =="
    if [ ! -f "$STATE_DIR/pre-login-environment.txt" ]; then
        echo "WARN: no saved baseline at $STATE_DIR/pre-login-environment.txt" >&2
        echo "      (did you run 'prepare' before this login?) - skipping the diff." >&2
    else
        u systemctl --user show-environment > "$STATE_DIR/post-logout-environment.txt" 2>&1

        # Confirmed root cause (see docs/known-issues.md): a real bug
        # in the Hyprland binary's own embedded shutdown command re-exports
        # these vars' stale values from its own process env in the same
        # breath it unsets them, regardless of how the session ends. Not a
        # wsmr defect - allowlisted here so a REAL new regression doesn't
        # get lost in noise this harness already knows the cause of.
        KNOWN_HYPRLAND_LEAKS="WAYLAND_DISPLAY XDG_CURRENT_DESKTOP XDG_SESSION_DESKTOP XDG_BACKEND XDG_MENU_PREFIX"

        ADDED=$(comm -13 \
            <(sort "$STATE_DIR/pre-login-environment.txt") \
            <(sort "$STATE_DIR/post-logout-environment.txt"))
        UNEXPECTED=""
        while IFS= read -r line; do
            [ -z "$line" ] && continue
            name=${line%%=*}
            case " $KNOWN_HYPRLAND_LEAKS " in
                *" $name "*)
                    echo "KNOWN ISSUE (not wsmr's fault - see docs/known-issues.md): $line still set" ;;
                *)
                    UNEXPECTED="$UNEXPECTED
$line" ;;
            esac
        done <<<"$ADDED"

        if [ -n "$UNEXPECTED" ]; then
            echo "FAIL: unexpected leftover variable(s) after logout, not on the known-issue list:"
            echo "$UNEXPECTED" | sed '/^$/d;s/^/  /'
            CHECKS_FAILED=$((CHECKS_FAILED + 1))
        else
            echo "PASS: no unexpected leftover variables (only the known, root-caused Hyprland ones, if any)"
        fi
        CHECKS_TOTAL=$((CHECKS_TOTAL + 1))
    fi

    echo
    echo "== post-logout: no stale wsmr runtime state =="
    check "no stale files under \$XDG_RUNTIME_DIR/wsmr (state.lock excepted)" \
        "[ ! -d '$RUNTIME_DIR/wsmr' ] || [ -z \"\$(sudo find '$RUNTIME_DIR/wsmr' -type f -not -name state.lock)\" ]"

    echo
    echo "== post-logout: summary =="
    echo "$((CHECKS_TOTAL - CHECKS_FAILED))/$CHECKS_TOTAL checks passed"
    [ "$CHECKS_FAILED" -eq 0 ]
}

case "$STAGE" in
    prepare) run_prepare ;;
    verify) run_verify ;;
    post-logout) run_post_logout ;;
esac
