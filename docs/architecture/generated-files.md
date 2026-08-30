# Generated-file safety

wsmr writes systemd units into either the runtime or user-configuration rung.
It must avoid overwriting user files and units installed by uwsm.

## Plan before apply

`src/units/plan.rs` reads the destination and builds a complete plan. The plan
contains writes, removals, conflicts, and any stale drop-ins eligible for
reclamation. It does not modify the filesystem.

Only after the plan has no blocking conflict does
`src/units/generate.rs` apply it. Writes use temporary files and atomic
renames. A failed batch rolls back earlier writes from that batch.

This separation gives `start --dry-run` the same conflict detection as a real
start without changing files or reloading systemd.

## Ownership manifest

The `.wsmr-generation` manifest records a fingerprint of every file wsmr
actually wrote. Before updating or deleting a tracked file, wsmr verifies that
its current content still matches the fingerprint.

If a tracked file was edited later, wsmr treats it as foreign and leaves it
alone.

## Shared static graph

wsmr and uwsm use the same static unit names and content. Identical static
content is accepted but not claimed as wsmr-owned. A static unit with different
content is a hard conflict.

Static graph units are never removed by `stop --remove`. Their identical
content cannot show which session manager installed them.

## Stale drop-in reclamation

Per-compositor `50_custom.conf` files and fixed tweak drop-ins are generated
paths rather than normal hand-edited configuration. wsmr may replace a foreign
file at one of these paths, but only after a systemd query confirms that no
session is active.

The reclamation is reported to the journal. It exists so a machine can switch
between uwsm-managed and wsmr-managed login entries without stale generated
drop-ins blocking the next session.

## Runtime-state locking

Environment state lives under `$XDG_RUNTIME_DIR/wsmr/`. Separate processes
prepare, finalize, watch readiness, and clean up the environment, so all
read-modify-write operations share one advisory file lock.

The kernel releases the lock if a process crashes. There is no stale lockfile
to remove.

Every session also receives a random generation ID. Cleanup entries carry that
ID, preventing an entry abandoned by an older session from being mistaken for
one created by the current session.

There is one narrow known race: an old `cleanup-env` process that is still
running when a new generation begins cannot carry its old generation ID across
the static unit boundary. `start` reduces the window by refusing to proceed
while a compositor unit is active or activating. The limitation is documented
in `src/session/state.rs`.
