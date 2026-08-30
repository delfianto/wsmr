# Getting started

## Before you install

wsmr expects an existing Linux desktop stack. You need:

- systemd and logind;
- a running systemd user manager;
- a D-Bus user session;
- `systemctl`, `systemd-cat`, and `systemd-notify`; and
- a standalone Wayland compositor.

wsmr does not provide a compositor or a display manager. It also does not
support non-Linux systems.

Only Hyprland has been tested on real hardware. Other compositors may work,
but should be treated as untested.

## Build and install

The crate requires Rust 1.98 or newer.

```sh
cargo build --release --locked
install -Dm755 target/release/wsmr ~/.local/bin/wsmr
```

The release profile uses thin LTO, one codegen unit, stripped symbols, and
`panic = "abort"`.

Arch users can also build the local package described in
[the Arch packaging guide](../arch/packaging.md).

## Configure readiness

The compositor service uses `Type=notify`. Before the graphical session can
start, `WAYLAND_DISPLAY` must reach the activation environment.

Some compositors publish it themselves. Otherwise, add `wsmr finalize` to a
compositor startup hook. For Hyprland:

```ini
exec-once = wsmr finalize
```

The automatic watcher observes activation-environment changes and sends the
readiness notification. It cannot read a variable that exists only inside the
compositor process.

## Start from a TTY

Run this from a real login shell on an allowed virtual terminal:

```sh
wsmr check may-start && exec wsmr start YOUR_COMPOSITOR
```

`check may-start` rejects common invalid contexts, such as an existing
graphical session or a remote login. By default it expects VT 1. Use its flags
only when your login setup requires different rules:

```sh
wsmr check may-start --help
```

## Start from a display manager

Create a file under `/usr/local/share/wayland-sessions/` or another directory
read by your display manager:

```ini
# /usr/local/share/wayland-sessions/sway-wsmr.desktop
[Desktop Entry]
Name=Sway (wsmr)
Exec=wsmr start /usr/bin/sway
Type=Application
```

The Arch package includes an equivalent Hyprland entry.

For Hyprland, add this environment file before relying on wsmr for daily use:

```sh
# ~/.config/wsmr/env-hyprland
export HYPRLAND_NO_SD_VARS=1
```

This prevents older Hyprland releases from independently changing the same
activation environment that wsmr manages. See
[Hyprland environment cleanup](known-issues/hyprland-environment.md).

When that variable disables Hyprland's own export, the `wsmr finalize` startup
hook above is required.

## Confirm the session

Once the compositor is running:

```sh
wsmr check is-active --verbose
systemctl --user status graphical-session.target
systemctl --user list-units 'wayland-*'
```

Use `wsmr app` for programs that should belong to the graphical session:

```sh
wsmr app firefox.desktop
wsmr app -- foot
```

## Stop the session

```sh
wsmr stop
```

The same cleanup runs when the compositor exits on its own. `wsmr stop -r`
also removes wsmr-owned compositor and tweak drop-ins; the shared static unit
graph is deliberately left in place.

If startup or cleanup fails, continue with [Recovery](recovery.md).
