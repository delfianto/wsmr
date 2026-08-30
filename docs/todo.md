# Open work

The core CLI is implemented. Native tests, the Tier B happy path, and all six
Tier B failure scripts pass. This page tracks the remaining verification and
test coverage; it is not an architecture document.

## Requires a real console

- [ ] **Crash after readiness.** Start a real Hyprland session in the
  disposable account, kill the compositor, and confirm that units and the
  activation environment are cleaned up. Tier B already covers the equivalent
  stub-compositor failure.
- [ ] **Display-manager cancellation.** Force the display manager to terminate
  a login in progress and confirm clean teardown.

The broken-compositor-configuration case has already been checked on hardware.
It fails before the compositor claims the display and recovers cleanly.

See [the hardware procedure](../arch/e2e.md).

## Automated test gaps

- [ ] Add a D-Bus-activatable fixture that proves environment propagation to a
  service which does not declare the variables itself.
- [ ] Add golden snapshots for the complete CLI. The current surface was
  checked against upstream source, but no snapshot prevents future drift.
- [ ] Exercise a stale app-daemon FIFO in Tier B. Unit tests cover FIFO reuse;
  Tier B currently covers a missing output reader.
- [ ] Add the merged coverage gate to CI. It passes locally above 92% and
  enforces a 90% minimum.

## Follow-up investigation

- [ ] Re-test the proposed portal shutdown-order drop-in across enough logout
  cycles to show whether it changes the intermittent crash. See
  [Portal crash during teardown](known-issues/portal-crash.md).
- [ ] Re-test Hyprland environment cleanup with a release containing the newer
  systemd-session behavior. It may make `HYPRLAND_NO_SD_VARS=1` unnecessary.

## Not planned yet

- Testing a second distribution. There is no packaging target beyond the
  local Arch package.
- Testing other compositors. Hyprland is the only compositor currently used in
  real-hardware verification.
