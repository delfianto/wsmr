# Known issues

This section records problems observed during real Hyprland sessions. The
container suite cannot expose DRM, input-seat, portal, or compositor-specific
shutdown behavior.

## Compositor support

| Compositor | Status |
|---|---|
| Hyprland | Tested on real hardware, including a greetd login and application launching. |
| sway, niri, river, labwc, and others | Not tested with wsmr. |

The session graph is compositor-independent, but that does not replace actual
hardware testing. Treat every compositor other than Hyprland as unsupported
test territory.

## Verified findings

- [kmscon can retain the seat and DRM device](known-issues/kmscon-seat-conflict.md)
  — reproducible with both wsmr and uwsm. Use a plain getty on the target VT.
- [Hyprland can leave activation variables behind](known-issues/hyprland-environment.md)
  — affects older Hyprland releases. `HYPRLAND_NO_SD_VARS=1` is the verified
  mitigation.
- [`xdg-desktop-portal-hyprland` can crash during logout](known-issues/portal-crash.md)
  — reproduced with both wsmr and uwsm. A proposed ordering drop-in remains
  unverified.
- [Hyprland itself crashed once during cleanup](known-issues/hyprland-cleanup-crash.md)
  — a separate one-off compositor crash that has not been reproduced.

## Test environment

The findings came from this system:

| Component | Version |
|---|---|
| Test date | 2026-08-29 |
| Distribution | CachyOS, Arch-based |
| Kernel | `7.2.2-1-cachyos` |
| systemd | `261.2-1` |
| D-Bus | `1.16.2-1.1` |
| Hyprland | `0.56.2-1` |
| xdg-desktop-portal-hyprland | `1.4.1-1.1` |
| wsmr | `0.1.0-1` |
| uwsm used for comparison | `0.26.7` |
| Display manager | greetd with noctalia-greeter-session |

Do not assume a workaround is necessary on newer component versions. Check the
linked upstream report and reproduce the behavior first.
