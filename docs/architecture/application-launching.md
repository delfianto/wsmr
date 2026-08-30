# Application launching

`wsmr app` resolves a user request into one or more `systemd-run --user`
commands.

## Resolution

The target may be a normal command, a desktop-entry path, or an ID found below
the XDG application directories. An `id:action` suffix selects a desktop-entry
action.

The parser checks `Type`, `Name`, `Exec`, `TryExec`, `OnlyShowIn`,
`NotShowIn`, and related fields needed by wsmr. It is deliberately smaller
than a general desktop-entry library.

`Exec=` tokenization and field expansion support `%f`, `%F`, `%u`, `%U`, `%c`,
`%k`, and `%i`. Multi-value fields can produce several commands, which wsmr
launches as separate units.

## Terminal wrapping

An entry with `Terminal=true`, or a request using `app --terminal`, is wrapped
in a terminal emulator. Resolution first checks `xdg-terminals.list`, then
scans desktop entries in the `TerminalEmulator` category.

## Unit construction

The default launch is a transient scope in `app-graphical.slice`. A service
launch adds `Type=exec`, `ExitType=cgroup`, and session-specific environment
values to the generated `systemd-run` arguments.

Automatic unit names include the desktop name, application name, and random
suffix. Names are escaped and truncated to systemd limits. Users may provide
an explicit unit name or extra unit properties.

## FIFO daemon

`wayland-wm-app-daemon.service` runs an optional argument-resolution server.
A client writes NUL-separated arguments to
`$XDG_RUNTIME_DIR/wsmr-app-daemon-in`; the daemon writes one shell command to
the output FIFO.

The output open has a five-second bound, so a missing client reader does not
hang the daemon. This protocol is internal and is primarily kept for uwsm
compatibility and thin external clients.
