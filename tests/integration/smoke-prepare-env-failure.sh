#!/usr/bin/env bash
# P4-03 scenario: prepare-env failure. wayland-wm-env@<id>.service's own
# ExecStart is `wsmr aux prepare-env`, a Type=oneshot -- a nonzero exit fails
# the unit immediately. Triggered here the same way a real user's broken
# config would: prepare-env.sh's generic env-file loader (load_wm_env,
# libexec/prepare-env.sh) sources "$XDG_CONFIG_HOME/wsmr/env" if present, in
# the *current* shell via `.`, so a bare `exit 1` in that file aborts the
# whole loader script, not just a subshell.
set -euo pipefail

WSMR="${WSMR:-/opt/wsmr-target/debug/wsmr}"
STUB="${STUB:-/opt/it/stub-compositor.sh}"

fail() { echo "FAIL: $1" >&2; exit 1; }

collect_diagnostics() {
    echo "---- diagnostics: failed units ----" >&2
    systemctl --user list-units --failed --no-legend >&2 || true
    echo "---- diagnostics: recent user journal (last 200 lines) ----" >&2
    journalctl --user -n 200 --no-pager >&2 || true
}
trap 'rc=$?; if [ "$rc" -ne 0 ]; then collect_diagnostics; fi' EXIT

echo "== capturing the pre-session activation environment baseline =="
PRE_ENV="$(systemctl --user show-environment)"

echo "== installing a broken \$XDG_CONFIG_HOME/wsmr/env (a realistic user config mistake) =="
mkdir -p "$HOME/.config/wsmr"
cat > "$HOME/.config/wsmr/env" <<'EOF'
# Deliberately broken: exits the *sourcing* shell (prepare-env.sh's
# load_wm_env loader), not a subshell, since this file is sourced via `.`.
echo "smoke-prepare-env-failure: simulating a broken user env file" >&2
exit 1
EOF

echo "== starting session (prepare-env should fail before the compositor ever runs) =="
set +e
timeout 30 "$WSMR" start "$STUB" >/tmp/wsmr-start.log 2>&1
RC=$?
set -e
echo "---- wsmr start exit code: $RC ----"
echo "---- wsmr start output ----"
cat /tmp/wsmr-start.log
[ "$RC" -ne 124 ] || fail "'wsmr start' hung for 30s instead of surfacing the prepare-env failure"
# Same reasoning as smoke-crash-before-readiness.sh: 'wsmr start' itself
# exits 0 (it's the start+shutdown *job* completing, not session success);
# failure detection is via unit-graph state, asserted below.
[ "$RC" -eq 0 ] \
    || fail "'wsmr start' exited $RC, diverging from the confirmed exit-0-on-this-failure-mode behavior"

# journald indexing can lag; retry briefly rather than treating that lag as a
# real failure, same as the sibling scenarios.
JOURNAL_OK=0
for _ in $(seq 1 10); do
    if journalctl --user -n 500 --no-pager 2>/dev/null | grep -q "wayland-wm-env@.*\.service: Main process exited" \
        && journalctl --user -n 500 --no-pager 2>/dev/null | grep -q "wayland-wm-env@.*\.service: Failed with result 'exit-code'"; then
        JOURNAL_OK=1
        break
    fi
    sleep 0.5
done
[ "$JOURNAL_OK" -eq 1 ] \
    || fail "journal never showed wayland-wm-env@'s prepare-env failure within 5s of 'wsmr start' returning"
echo "PASS: journal confirms wayland-wm-env@ failed (prepare-env exited nonzero)"

# NOTE: the broken env file's own stderr message ("simulating a broken user
# env file") never reaches the journal, and this is worth flagging as its
# own minor finding, not a bug in this test: `session::prepare::run_loader`
# captures the loader shell's stdout/stderr via `Command::output()` (not
# inherited), then calls `dump::parse_shell_dump(&stdout, mark)?` *before*
# checking `output.status.success()`. Since the broken config file's `exit
# 1` aborts the loader shell before it ever prints the closing random-mark
# boundary, `parse_shell_dump` fails first with its own generic message
# ("could not resolve env output mark ... not found in shell output" --
# confirmed via a live run) and returns early via `?`, so the branch that
# would have included `output.stderr` (and thus the real reason) in the
# error is never reached. The unit still fails correctly and the whole
# graph still tears itself down correctly (asserted below) -- only the
# specific *diagnostic text* a real user would see for this failure mode is
# less useful than it could be. Confirmed instead via the generic signal
# that's actually reliable: the mark-resolution failure itself.
JOURNAL_OK=0
for _ in $(seq 1 10); do
    if journalctl --user -n 500 --no-pager 2>/dev/null | grep -qi "could not resolve env output mark"; then
        JOURNAL_OK=1
        break
    fi
    sleep 0.5
done
[ "$JOURNAL_OK" -eq 1 ] \
    || fail "journal never showed the expected 'could not resolve env output mark' failure -- prepare-env may have failed for a different, unexpected reason"
echo "PASS: prepare-env failed for the expected reason (the loader shell exited early, before printing its closing mark)"

echo "== asserting the compositor's own unit never started (ordering held) =="
if journalctl --user -n 500 --no-pager 2>/dev/null | grep -q "wayland-wm@.*\.service: Starting"; then
    fail "wayland-wm@ was started despite its prepare-env dependency failing"
fi
echo "PASS: the compositor's own service never started"

echo "== asserting the session never reached readiness =="
[ "$(systemctl --user is-active graphical-session.target 2>&1)" != active ] \
    || fail "graphical-session.target became active despite the prepare-env failure"
systemctl --user show-environment | grep -q '^WAYLAND_DISPLAY=' \
    && fail "WAYLAND_DISPLAY was exported despite prepare-env never completing"
echo "PASS: graphical-session.target never activated, WAYLAND_DISPLAY never exported"

echo "== asserting the pre-readiness graph tore itself back down (OnFailure=wayland-session-shutdown.target cascade) =="
for _ in $(seq 1 20); do
    [ "$(systemctl --user is-active graphical-session-pre.target 2>&1)" != active ] && break
    sleep 0.3
done
[ "$(systemctl --user is-active graphical-session-pre.target 2>&1)" != active ] \
    || fail "graphical-session-pre.target is still active after the prepare-env failure"
echo "PASS: the shutdown cascade correctly tore down the rest of the pre-readiness graph"

rm -f "$HOME/.config/wsmr/env"
systemctl --user reset-failed >/dev/null 2>&1 || true

POST_ENV="$(systemctl --user show-environment)"
[ "$PRE_ENV" = "$POST_ENV" ] || fail "activation environment was not fully restored after the prepare-env failure"
echo "PASS: activation environment matches the pre-session baseline after the prepare-env failure"
