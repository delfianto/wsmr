# kmscon seat conflict

## Summary

On the tested CachyOS system, switching to an unused VT started `kmscon`
through an `autovt@.service` alias. Starting Hyprland from that console left
kmscon and the compositor competing for the seat and DRM device.

The result was a running desktop with unusable mouse and keyboard input. A
comparison run through uwsm made Hyprland fail during DRM backend creation.
The problem is not specific to wsmr.

## Check whether the system uses kmscon

```sh
systemctl cat autovt@.service
```

If it resolves to `kmsconvt@.service`, an unused VT may start a KMS console
instead of a normal getty.

## Workaround

Start a plain getty on an unused VT before switching to it:

```sh
sudo systemctl start getty@tty2.service
```

Replace `tty2` with the VT you intend to use. logind then finds that VT already
claimed and does not start kmscon there.

This workaround removed the repeated seat enable/disable loop in Hyprland's
log and restored normal input on the tested machine.

## Why wsmr's handoff is not enough

The embedded `signal-handler.sh` contains the same kmscon handoff used by
uwsm. `start` also preserves file descriptors 3 and 4 as required by that
script. The live comparison showed the same underlying failure with both
session managers, so the remaining conflict is between kmscon and the
compositor's DRM/seat handling.

The related upstream report is
[Hyprland issue 7423](https://github.com/hyprwm/Hyprland/issues/7423).

## Display-manager sessions

The tested greetd setup owns VT1 directly, so it does not trigger the unused-VT
activation path. That explains why normal greetd logins did not show this
problem, but the mechanism has not been isolated in a separate controlled
test.
