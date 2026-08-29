#!/bin/sh
# Fake multi-instance app for wsmr integration tests: reads the file it's
# given, proving the desktop entry's `%f` substitution actually happened
# per launched instance (not just "the unit started"), then idles.
[ -r "$1" ] || { echo "multi-instance-app: cannot read '$1'" >&2; exit 1; }
cat "$1" > /dev/null
exec sleep 600
