#!/bin/sh
# Fake graphical app for wsmr integration tests: touches a marker file so a
# desktop-entry launch's *effect* is observable, not just its unit state,
# then idles so the launching test has time to inspect the running unit
# (PID/slice) before the session teardown reaps it.
touch /tmp/wsmr-marker-desktopapp
exec sleep 600
