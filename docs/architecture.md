# Architecture

wsmr turns one compositor command into a systemd user-session graph. systemd
then owns process lifetime, ordering, application slices, and shutdown.

The design has three important boundaries:

1. Unit generation is planned before files are changed.
2. Environment changes are calculated from before-and-after snapshots.
3. systemd and D-Bus operations sit behind small interfaces so most decisions
   can be tested without a live session.

![Overview of the generated unit graph](diagrams/unit-graph.svg)

## Focused design pages

- [Unit graph](architecture/unit-graph.md) — generated units and dependency
  roles.
- [Session lifecycle](architecture/session-lifecycle.md) — the process flow
  from `start` through readiness and cleanup.
- [Environment management](architecture/environment.md) — snapshots, delta
  rules, environment files, and restoration.
- [Generated-file safety](architecture/generated-files.md) — planning,
  manifests, coexistence, and runtime generations.
- [Application launching](architecture/application-launching.md) — desktop
  entries, terminal wrapping, scopes, services, and the FIFO daemon.

## Source map

| Path | Role |
|---|---|
| `src/session/` | Session lifecycle and state. |
| `src/units/` | Unit templates and plan/apply logic. |
| `src/env/` | Environment snapshots and delta calculation. |
| `src/sysd/dbus.rs` | systemd, logind, notification, and D-Bus calls. |
| `src/app/` | Application resolution and launching. |
| `src/comp.rs` | Compositor command and desktop-entry resolution. |
| `src/varnames.rs` | Environment-variable policy. |
| `libexec/` | Embedded shell helpers. |

## Design choices

### Let systemd manage the session

wsmr does not keep a private process supervisor. It generates units, starts an
envelope target, and lets systemd dependencies propagate startup and shutdown.

### Use blocking D-Bus calls

The code uses blocking zbus interfaces. The workflow is mostly sequential and
does not need an async runtime.

### Generate the full graph

uwsm installs most static units as package data. wsmr embeds the templates and
generates them at runtime. The binary and the units it expects cannot drift
apart.

### Keep compositor policy outside the core

The core does not contain Hyprland-, sway-, or niri-specific branches.
Compositor quirks belong in environment files or packaging. See
[Configuration](configuration.md).

For differences from upstream, see [Compatibility with uwsm](uwsm-compatibility.md).
