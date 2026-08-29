# Coexistence with uwsm

wsmr is a Rust port of uwsm and, by design, uses the **same systemd user-unit
names** as uwsm: `wayland-wm@.service`, `wayland-wm-env@.service`, the
`wayland-session-*` targets, and per-compositor `wayland-wm@<id>.service.d/`
drop-in directories. This is intentional — it's what makes wsmr a drop-in
replacement instead of a parallel, incompatible session stack. It also means
uwsm and wsmr can, at any point, be pointed at the exact same unit directory
(`$XDG_RUNTIME_DIR/systemd/user` or `$XDG_CONFIG_HOME/systemd/user`).

Sharing a namespace with another tool that can write to it means wsmr can
never assume a file at one of these paths is its own just because the name
matches. The rest of this document is the policy that follows from that.

## Ownership is proven, never assumed

Every path wsmr writes is tracked in a per-directory manifest
(`.wsmr-generation`, see [`src/units/manifest.rs`](../src/units/manifest.rs))
that records a content fingerprint of exactly what wsmr wrote. A path counts
as wsmr-owned only when **both**:

1. the manifest lists it, and
2. the file on disk still has that exact fingerprint.

Anything else — a file the manifest doesn't mention, or one whose content has
drifted since wsmr last wrote it (hand-edited, or overwritten by another
tool) — is treated as foreign. wsmr never overwrites or deletes a foreign
file. See [`src/units/plan.rs`](../src/units/plan.rs) for the classification
logic and [`src/units/generate.rs`](../src/units/generate.rs) for how a
conflict is reported (exact path + reason, never silently skipped or merged).

## The static graph is never auto-removed

The graph units in [`src/units/templates.rs`](../src/units/templates.rs)
(`GRAPH`) are kept byte-identical to upstream uwsm's own units. That's
deliberate — it's what lets the two be diffed against each other — but it
also means **content alone can never tell wsmr's copy of a graph unit apart
from uwsm's copy**: if both tools would render the same bytes, the file looks
identical regardless of who last wrote it.

Because of that, `wsmr stop --remove` (and any future "uninstall" path) only
ever removes per-compositor `50_custom.conf` drop-ins that pass ownership
verification. It never deletes the 13 static graph files, even if a stale
manifest somehow claims one. Leaving them behind is harmless: they're inert
without an active compositor, and regenerating them is an idempotent no-op.

## Foreign-space removal is a no-op, not an error

If wsmr would normally clean up a drop-in (e.g. the compositor no longer
needs customization) but the file at that path isn't verifiably wsmr's, wsmr
leaves it alone silently. Overwriting foreign content is always refused
outright (it blocks the whole generation); *not* deleting foreign content
is never an error — there's nothing unsafe about leaving a file wsmr doesn't
own exactly where it was.

## What this means in practice

- Starting wsmr while uwsm (or another wsmr generation) already occupies a
  path wsmr needs to write refuses with a diagnostic listing the exact
  conflicting paths — it does not overwrite, adopt, or merge.
- `wsmr stop --remove` only removes what wsmr can prove it wrote; it's safe
  to run even while uwsm is coexisting in the same unit directory.
- There is currently no adoption/migration flow that turns a foreign file
  into a wsmr-owned one. If wsmr refuses to start because of a conflict, the
  fix is to stop the other tool/session first, or to manually remove the
  conflicting file once you've confirmed it's safe to do so. An explicit,
  opt-in adoption command is future work — wsmr will not do this implicitly.
