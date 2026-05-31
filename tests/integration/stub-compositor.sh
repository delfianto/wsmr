#!/bin/sh
# Stub Wayland compositor for wsmr integration tests.
#
# A real compositor would create a Wayland socket and put WAYLAND_DISPLAY into
# the systemd/D-Bus activation environment. This stub just does the latter (so
# wsmr's autoready watcher declares the unit ready) and then idles until the
# session is stopped.
systemctl --user set-environment WAYLAND_DISPLAY=wayland-stub
exec sleep infinity
