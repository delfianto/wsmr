#!/bin/sh
# Deliberately-broken stub compositor for the "compositor exits before
# readiness" P4-03 scenario: exits with a nonzero status before ever
# creating the Wayland socket or calling `wsmr finalize`, simulating a real
# compositor crashing during its own startup (before readiness).
set -eu

echo "stub-compositor-crash: simulating a startup crash before readiness" >&2
exit 17
