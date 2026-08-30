# Configuration

wsmr has no monolithic configuration file. Configuration comes from command
options, environment files, and a small set of environment variables.

## Compositor environment files

During `prepare-env`, wsmr loads shell files from the XDG configuration and
system data directories. Lower-priority directories are loaded first, so user
configuration wins.

For each directory, it loads:

1. `wsmr/env` and `wsmr/env.d/*`;
2. `wsmr/env-<desktop>` and `wsmr/env-<desktop>.d/*` for every lowercased name
   in `XDG_CURRENT_DESKTOP`.

For a Hyprland session, the usual user file is:

```sh
# ~/.config/wsmr/env-hyprland
export HYPRLAND_NO_SD_VARS=1
export MY_SESSION_SETTING=value
```

If `HYPRLAND_NO_SD_VARS` disables Hyprland's own activation-environment
updates, configure `exec-once = wsmr finalize` in Hyprland as well.

These are POSIX shell files, not `KEY=value` parsers. They are sourced into the
environment-loader shell, so syntax errors or `exit` will fail session
preparation.

Loaded values pass through the normal environment-delta logic and are removed
or restored when the session ends.

## Supported environment variables

| Variable | Purpose |
|---|---|
| `UWSM_UNIT_RUNG` | Default unit location: `run` or `home`. |
| `UWSM_TWEAKS` | Enable or disable standard tweak drop-ins. |
| `UWSM_NO_TWEAKS` | Deprecated inverse of `UWSM_TWEAKS`. |
| `UWSM_APP_UNIT_TYPE` | Default `app` unit type: `scope` or `service`. |
| `UWSM_FINALIZE_VARNAMES` | Space-separated variables added by `finalize`. |
| `UWSM_WAIT_VARNAMES` | Extra variables required before readiness. |
| `UWSM_WAIT_VARNAMES_TIMEOUT` | Readiness timeout in seconds; default 30. |
| `UWSM_WAIT_VARNAMES_SETTLETIME` | Delay after variables appear; default 0.2 seconds. |

The `UWSM_` prefix is retained for compatibility with upstream uwsm.

## Generated file locations

The default unit rung is `run`:

```text
$XDG_RUNTIME_DIR/systemd/user/
```

`start -U home` instead uses:

```text
$XDG_CONFIG_HOME/systemd/user/
```

Session state is stored under:

```text
$XDG_RUNTIME_DIR/wsmr/
```

That directory contains environment snapshots, cleanup records, the current
generation ID, and the state lock. It is runtime state, not user
configuration.
