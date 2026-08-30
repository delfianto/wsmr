# Portal crash during compositor teardown

## Summary

`xdg-desktop-portal-hyprland` 1.4.1 crashed intermittently while Hyprland
0.56.2 was shutting down. The failure reproduced under both wsmr and uwsm.

This is a portal teardown bug, not a difference in the session-manager graph.

## Observed sequence

The journal showed the portal receiving a new Wayland registry interface while
the compositor was dismantling its globals. About 130 microseconds later, the
portal exited with `SIGSEGV`.

Its `Restart=on-failure` policy then retried after the Wayland socket was gone.
Those attempts failed to connect until systemd stopped retrying with
`start-limit-hit` and six recorded restarts.

Three other user services also ended in a failed state during the same logout:

- `xdg-desktop-portal-gtk.service`;
- `app-blueman@autostart.service`; and
- `app-cachyos-hello@autostart.service`.

Those three failures were not investigated to the same depth. The two
autostart services simply returned exit status 1 while their graphical slice
was stopping.

The matching upstream report is
[xdg-desktop-portal-hyprland issue 330](https://github.com/hyprwm/xdg-desktop-portal-hyprland/issues/330).

## Proposed mitigation: not verified

The following drop-in attempts to stop the portal services before systemd
signals Hyprland:

```ini
# ~/.config/systemd/user/wayland-wm@hyprland.desktop.service.d/xdph-teardown-order.conf
[Service]
ExecStop=-/usr/bin/systemctl --user stop xdg-desktop-portal-hyprland.service xdg-desktop-portal-gtk.service xdg-desktop-portal.service
```

Two sessions ended cleanly with the drop-in installed, but that does not prove
it fixed the race. The portal services had already stopped before the
compositor unit's stop action could be shown to have caused the ordering. The
original crash was also intermittent.

Treat this as an experiment, not a recommended fix. If you test it, verify the
unit timeline in the journal rather than counting a few crash-free logouts.
