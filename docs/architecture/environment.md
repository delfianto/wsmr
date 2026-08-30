# Environment management

A compositor and D-Bus-activated desktop services need the same session
variables. wsmr updates both the systemd user-manager environment and the
D-Bus activation environment, then restores their previous state at logout.

![Environment snapshot and cleanup flow](../diagrams/env-delta.svg)

## Preparation

`prepare-env` starts with two snapshots:

- `env_pre`: the systemd activation environment before session preparation;
- `env_post`: the environment produced by the shell loader.

The loader may read `/etc/profile`, `~/.profile`, and wsmr environment files.
It also establishes standard session values, including:

```text
XDG_CURRENT_DESKTOP
XDG_SESSION_DESKTOP
XDG_MENU_PREFIX
XDG_SESSION_TYPE=wayland
XDG_BACKEND=wayland
```

`XDG_BACKEND` is explicitly set in `libexec/prepare-env.sh`; it is not an
unexplained external value.

## Delta rules

`src/env/delta.rs::compute_changes` calculates three sets:

- **set**: values that are new or changed after loading, plus values that must
  always be exported;
- **unset**: pre-session values missing afterward, plus variables that must
  never leak into the shared activation environment; and
- **cleanup**: exported names that must be undone at logout.

The variable classes live in `src/varnames.rs`.

Seat and logind identity values such as `XDG_SESSION_ID` and `XDG_VTNR` are
session-specific. They are passed to relevant units, but not exported into the
shared activation environment.

SSH agent variables may be exported but are protected from session cleanup.

## Environment-file order

The shell loader walks this combined hierarchy:

```text
XDG_CONFIG_HOME : XDG_CONFIG_DIRS : XDG_DATA_DIRS
```

It processes the list in reverse, from lowest to highest priority. Within each
directory it loads the common `wsmr/env` file first, followed by files for the
desktop names. Each file's matching `.d/` directory follows the file.

For details and examples, see [Configuration](../configuration.md).

## Readiness-time updates

The compositor normally creates `WAYLAND_DISPLAY` after environment
preparation. The readiness watcher snapshots the activation environment,
waits for required variables, and records any values that appeared while it
was waiting.

`wsmr finalize` performs a similar operation from inside the compositor. Both
paths record cleanup obligations before exporting values. If the export then
fails, cleanup has harmless extra work; the reverse order could leak a value
with no cleanup record.

## Restoration

At stop, cleanup:

1. reads the pre-session snapshot;
2. selects cleanup entries belonging to the current generation;
3. removes session-only values that were not in the snapshot;
4. restores the old values; and
5. deletes the completed generation's state files.

A new generation resolves abandoned old state before saving its own snapshot.
This handles crashes that prevented the previous `ExecStopPost` from running.
