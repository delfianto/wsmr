#!/bin/sh
# Fake terminal emulator for wsmr integration tests (P4-01).
#
# Real terminal emulators (foot, alacritty, xterm, ...) support `-e CMD
# [ARGS...]`: run CMD (with ARGS) as the terminal's child instead of an
# interactive shell — that's what the paired `wsmrterm.desktop` fixture's
# `TerminalArgExec=-e` tells wsmr to build. Plain `/bin/sh` does *not*
# understand `-e` that way (`sh -e true` sets errexit and then tries to
# *open a script file* named "true" — this is the literal "shell unable to
# open true" failure the finding named), so a bare shell was never a valid
# terminal fixture. This one is: it records every argument it's invoked
# with, then finds "-e" and execs everything after it for real, so both the
# invocation *and* the payload's own success/failure are observable.
LOG="${WSMR_FAKE_TERM_LOG:-/tmp/wsmr-fake-term.log}"
printf '%s\n' "$*" >>"$LOG"

while [ "$#" -gt 0 ]; do
    arg="$1"
    shift
    if [ "$arg" = "-e" ]; then
        exec "$@"
    fi
done

echo "fake-terminal: no -e CMD found in arguments (invocation logged to $LOG)" >&2
exit 1
