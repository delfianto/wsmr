# Recovery

The tested failure cases recover without manual state repair. After a
compositor crash, readiness timeout, interrupted start, or environment-loader
failure, wait for systemd to finish teardown and start the session again.

## First response

From a TTY or another login session:

```sh
wsmr check is-active --verbose
wsmr stop
wsmr start YOUR_COMPOSITOR
```

`wsmr stop` is safe when no session is running; it becomes a no-op.

## Inspect a failed start

```sh
systemctl --user --failed
systemctl --user list-units 'wayland-*'
journalctl --user -b -t wsmr
journalctl --user -b -u 'wayland-wm@*' -u 'wayland-wm-env@*'
```

Common causes are:

- an error in `~/.config/wsmr/env` or a compositor-specific environment file;
- an existing graphical session;
- a foreign systemd unit at a path wsmr needs to generate;
- the compositor exiting before readiness; or
- missing `WAYLAND_DISPLAY` or another required readiness variable.

Run `wsmr start --dry-run ...` to inspect unit conflicts without changing the
systemd user manager.

## Remove generated drop-ins

If you want to discard wsmr-owned compositor customizations:

```sh
wsmr stop --remove
```

This does not delete the shared static unit graph or foreign files. A file
that changed after wsmr wrote it is left in place and reported.

## Leftover runtime state

A new session resolves an abandoned previous environment generation before it
creates a new one. In normal recovery, do not delete
`$XDG_RUNTIME_DIR/wsmr/` by hand; doing so would remove the data needed to
restore the previous activation environment.

If a problem is reproducible, save the user journal and generated unit files
before changing them. Then compare the symptoms with [Known issues](known-issues.md).
