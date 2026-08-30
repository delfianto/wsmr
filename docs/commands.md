# Commands

The generated `--help` output is the source of truth for every option. This
page explains when to use each public command.

## `start`

```sh
wsmr start [OPTIONS] COMPOSITOR [ARGS...]
```

`COMPOSITOR` may be an executable, a path, or a Wayland session desktop-entry
ID. wsmr resolves it, generates the required systemd user units, and starts the
session.

Common options:

| Option | Meaning |
|---|---|
| `-n`, `--dry-run` | Show the complete generation plan without writing files or starting a session. |
| `-o`, `--only-generate` | Write or update the units, then exit. |
| `-D NAMES` | Set colon-separated desktop names such as `Hyprland` or `sway`. |
| `-e`, `--exclusive` | Use only the names supplied with `-D`. |
| `-F`, `--hardcode` | Put the compositor's resolved absolute path in the generated unit. |
| `-U run\|home` | Write units to the runtime or user-configuration systemd directory. |
| `-t`, `--no-tweaks` | Disable the standard tweak drop-ins. |
| `-g SECONDS` | Warn if the system has not reached `graphical.target`. |
| `-G SECONDS` | Abort instead of warning. |

Use `wsmr start --help` for metadata options and exact defaults.

## `stop`

```sh
wsmr stop
wsmr stop --dry-run --remove
wsmr stop --remove hyprland.desktop,tweaks
```

Without options, `stop` stops the active compositor unit and lets the systemd
dependency graph perform cleanup.

`--remove` also removes generated drop-ins that wsmr can prove it owns. An
optional comma-separated filter limits removal to compositor IDs or `tweaks`.
The upstream `generic` mark is accepted for compatibility, but wsmr has no
removable files with that mark.

## `finalize`

```sh
wsmr finalize [VAR ...]
```

This command is for compositor startup hooks. It exports `WAYLAND_DISPLAY`,
`DISPLAY`, and the named variables to the activation environments, records
them for cleanup, and signals systemd that the compositor is ready.

The automatic watcher can observe variables that a compositor publishes to
the activation environment, but it cannot read changes made only inside the
compositor process. Use `finalize` when the compositor does not publish those
variables itself. Calling it from a compositor startup hook also gives more
precise readiness timing.

## `app`

```sh
wsmr app firefox.desktop
wsmr app firefox.desktop:new-window
wsmr app -- mpv ~/video.mkv
wsmr app -T -- btop
wsmr app -t service -s b -- syncthing
```

The target can be:

- a desktop-entry ID;
- a desktop-entry path;
- an entry action in `id:action` form; or
- a normal executable and its arguments.

wsmr expands desktop-entry `Exec=` field codes and uses `systemd-run --user`
to create a transient unit. The default is a scope in
`app-graphical.slice`.

Slice shortcuts:

| Value | Slice |
|---|---|
| `a` | `app-graphical.slice` |
| `b` | `background-graphical.slice` |
| `s` | `session-graphical.slice` |
| `name.slice` | the named custom slice |

Use `--type service` for a managed service. `--terminal` resolves a terminal
from `xdg-terminals.list`, then falls back to desktop entries in the
`TerminalEmulator` category.

## `check`

`wsmr check is-active [WM]` exits with status 0 when a graphical session, or
the requested compositor instance, is active. It exits with status 1
otherwise.

`wsmr check may-start` checks the login shell, VT, remote-session state,
system target, and existing graphical units. It is intended for login scripts.
Use `--verbose` to report all failed checks.

## `aux`

Commands below `aux` are implementation details used by generated units:

```text
prepare-env  cleanup-env  exec  waitpid  waitenv  app-daemon
```

`readiness` is also internal and hidden from normal help. Do not call these
commands manually unless you are debugging a generated unit.
