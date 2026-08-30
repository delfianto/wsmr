# Session lifecycle

![Session process and unit lifecycle](../diagrams/session-lifecycle.svg)

## 1. Plan the start

`wsmr start` performs read-only checks first:

1. optionally wait for the system `graphical.target`;
2. reject an already-active graphical session; and
3. compute the complete unit-file plan and identify conflicts.

`--dry-run` stops here after printing the plan. A normal start then applies
the plan and reloads the user manager only when files changed.

## 2. Create the session anchor

wsmr starts `wayland-session-bindpid@<pid>.service`, saves the login
environment, and preserves its original stdout and stderr on file descriptors
3 and 4.

It then replaces itself with:

```text
systemd-cat -- /bin/sh signal-handler.sh <envelope-target>
```

The shell process is the session anchor. It starts and waits for the envelope
target and handles the VT handoff messages inherited from uwsm.

## 3. Prepare the environment

`wayland-wm-env@.service` runs `wsmr aux prepare-env`. That process:

- fills in seat and logind session information;
- saves the activation environment for later restoration;
- runs the embedded `prepare-env.sh`; and
- applies the calculated environment delta.

See [Environment management](environment.md) for the rules.

## 4. Start the compositor

`wayland-wm@.service` runs `wsmr aux exec`. The helper starts a separate
readiness-watcher process, then replaces itself with the compositor command.
The compositor therefore becomes the main process of the systemd service.

wsmr starts a new watcher process instead of forking. A forked child inherited
zbus reactor state that no longer worked; a fresh process gets its own valid
D-Bus connection.

## 5. Signal readiness

The automatic watcher waits for `WAYLAND_DISPLAY` and any names in
`UWSM_WAIT_VARNAMES` to appear in the activation environment. A compositor or
one of its startup hooks must publish those values. After the settle delay,
the watcher records newly visible variables and calls `systemd-notify` with
`READY=1`.

A compositor may call `wsmr finalize` instead. That command exports variables
from the compositor process and sends the same readiness notification.

Once the notify service is ready, the graph reaches
`graphical-session.target` and starts XDG autostart.

## 6. Stop and restore

`wsmr stop` stops the active compositor service. A compositor exit or crash
also activates the shutdown target through `OnSuccess=` or `OnFailure=`.

As the graph stops, `wayland-wm-env@.service` runs `aux cleanup-env` from
`ExecStopPost`. Cleanup restores the pre-session activation environment and
removes variables introduced by this session generation.
