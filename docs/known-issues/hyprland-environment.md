# Hyprland activation-environment cleanup

## Summary

Hyprland 0.56.2 changed the systemd and D-Bus activation environments on its
own. Its shutdown command unset some values in systemd, then passed bare names
to `dbus-update-activation-environment`. Because the child process still had
those values, it exported them again.

The main visible leftovers included `WAYLAND_DISPLAY` and
`XDG_CURRENT_DESKTOP`. The same behavior occurred under uwsm, so it was not a
wsmr cleanup failure.

## Verified mitigation

Tell Hyprland to leave activation-environment management to the session
manager:

```sh
# ~/.config/wsmr/env-hyprland
export HYPRLAND_NO_SD_VARS=1
```

With Hyprland's exporter disabled, add this to the Hyprland configuration so
wsmr receives `WAYLAND_DISPLAY` and signals readiness:

```ini
exec-once = wsmr finalize
```

The environment-file mitigation was tested from a clean baseline. Hyprland
received the variable, wsmr tracked it as session-scoped state, and all session
values were removed after stop—including `HYPRLAND_NO_SD_VARS` itself.

The upstream background is in
[Hyprland issue 7083](https://github.com/hyprwm/Hyprland/issues/7083) and
[pull request 7358](https://github.com/hyprwm/Hyprland/pull/7358).

## Correction to the original investigation

An earlier version of this documentation called the origin of `XDG_BACKEND`
unknown. The source is explicit:

```sh
# libexec/prepare-env.sh
export XDG_BACKEND="wayland"
```

The same loader also sets `XDG_CURRENT_DESKTOP`, `XDG_SESSION_DESKTOP`, and
`XDG_MENU_PREFIX`. wsmr's cleanup policy includes the standard desktop
variables. `XDG_BACKEND` is handled through the normal computed delta when it
was introduced by the loader.

## Newer Hyprland behavior

[Hyprland pull request 15776](https://github.com/hyprwm/Hyprland/pull/15776)
changed activation-environment handling when Hyprland runs as a systemd
service. The hardware test described here predates that change. Re-test a
release containing it before deciding whether the environment-file workaround
is still needed.
