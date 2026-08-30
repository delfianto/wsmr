# wsmr

[![CI](https://github.com/delfianto/wsmr/actions/workflows/ci.yml/badge.svg)](https://github.com/delfianto/wsmr/actions/workflows/ci.yml)

wsmr is a Rust port of the core of
[uwsm](https://github.com/Vladimir-csp/uwsm). It runs a standalone Wayland
compositor as a systemd user session.

In practical terms, wsmr:

- starts the compositor in a systemd unit;
- updates the systemd and D-Bus activation environments;
- starts the standard graphical-session and XDG autostart targets;
- launches applications in session-owned scopes or services; and
- restores the activation environment when the session ends.

wsmr is for Linux systems that already use systemd, D-Bus, and a standalone
Wayland compositor. It is not a desktop environment or a compositor chooser.

> [!WARNING]
> wsmr is young. Its automated test suite is extensive, and Hyprland has been
> tested on real hardware, but no other compositor has been tested with wsmr.
> Read [Known issues](docs/known-issues.md) before using it for a real login.

## Quick start

Requirements:

- Linux with systemd, logind, and a working systemd user manager;
- a D-Bus user session;
- a Wayland compositor; and
- Rust 1.98 or newer to build from source.

Build a portable release binary:

```sh
cargo build --release --locked
```

The result is `target/release/wsmr`. To install it for your user:

```sh
install -Dm755 target/release/wsmr ~/.local/bin/wsmr
```

The compositor must either publish `WAYLAND_DISPLAY` to the activation
environment or run `wsmr finalize` from a startup hook. Once that is configured,
a typical TTY start is:

```sh
wsmr check may-start && exec wsmr start YOUR_COMPOSITOR
```

A display manager can start the same session through a desktop entry:

```ini
[Desktop Entry]
Name=Sway (wsmr)
Exec=wsmr start /usr/bin/sway
Type=Application
```

See [Getting started](docs/getting-started.md) for setup details and the
Hyprland-specific note.

## Common commands

```sh
wsmr start sway                  # start a compositor session
wsmr stop                       # stop the current session
wsmr app firefox.desktop        # launch a desktop entry
wsmr app -- mpv ~/video.mkv     # launch a command
wsmr check is-active            # exit 0 while a session is active
wsmr check may-start            # check whether this shell may start one
```

`finalize` is intended for compositor startup hooks. Commands below `aux` are
internal helpers used by generated systemd units.

The complete command reference is in [Commands](docs/commands.md).

## Project scope

wsmr implements the core session-management CLI from uwsm: `start`, `stop`,
`finalize`, `app`, session checks, and the internal helpers required by its
unit graph.

It intentionally does not implement uwsm's compositor selector, shell plugin
collection, `fumon`, or `ttyautolock`. Your login script or display manager
chooses the compositor.

The generated static unit graph matches uwsm 0.26.7 byte for byte. wsmr
generates that graph at runtime instead of installing it as package data.

## Development

The normal local checks are:

```sh
just typecheck
just full-gate
```

The systemd integration suite runs in Podman because it boots systemd as PID
1:

```sh
just integration
```

See [Development](docs/development.md) and [Testing](docs/testing.md) for the
full command list and test tiers.

## Documentation

The [documentation index](docs/README.md) separates user setup, command
reference, architecture, testing, compatibility, and real-hardware findings.

Useful starting points:

- [Getting started](docs/getting-started.md)
- [Configuration](docs/configuration.md)
- [Architecture](docs/architecture.md)
- [Known issues](docs/known-issues.md)
- [Open work](docs/todo.md)

## License

wsmr is licensed under the [MIT License](LICENSE). The two embedded POSIX
helpers are adapted from uwsm and retain its MIT copyright; see
[THIRD-PARTY-LICENSES](THIRD-PARTY-LICENSES).
