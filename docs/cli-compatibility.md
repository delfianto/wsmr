# CLI compatibility with uwsm 0.26.7 (P2-04)

Verified against the actual uwsm 0.26.7 package installed on the development
host (`/usr/share/uwsm/modules/uwsm/main.py`), not from memory — every claim
below traces to a specific line in that file. `check is-active` was also
verified live against that host's real, running uwsm-managed Hyprland
session (`wayland-wm@hyprland.desktop.service`).

## `start`

| Flag | uwsm | wsmr | Notes |
|---|---|---|---|
| `-o` / `--only-generate` | ✓ | ✓ | |
| `-n` / `--dry-run` | ✓ | ✓ | Strictly read-only in wsmr (P0-02) — upstream's dry-run still writes/reloads units, just exits before starting. Documented divergence, not a bug: see `fix-plan.md` Phase 0. |
| `-N` / `-C` / `-D` | ✓ | ✓ | |
| `-e` / `--exclusive` | ✓ | ✓ | |
| `-a` | Explicit opposite of `-e` (sets `desktop_names_exclusive=false`, already the default) | ✓ (`--append`) | Fixed this phase — wsmr previously mapped `-a` to hardcode. |
| `-F` / `--hardcode` | ✓ | ✓ | Fixed this phase — wsmr previously had no `-F`; `-a` did this instead. |
| `-U` / `--unit-rung {run,home}` | ✓, default from `$UWSM_UNIT_RUNG` (invalid value warns, falls back to `run`) | ✓ | Fixed this phase — wsmr previously used `runtime`/`home` as the value strings with no env var. `runtime` is kept as a value alias for wsmr's own prior spelling; `$UWSM_UNIT_RUNG` only accepts `run`/`home`, matching upstream exactly. |
| `-t` / `-T` (tweaks) | ✓, default from `$UWSM_TWEAKS`/deprecated `$UWSM_NO_TWEAKS` | ✓ | Implemented this phase, including the 3 fixed tweak drop-ins (`app-@autostart.service.d/slice-tweak.conf`, `app-flatpak-.scope.d/order-tweak.conf`, `plasma-xdg-desktop-portal-kde.service.d/order-tweak.conf`) ported verbatim from `generate_tweaks` (`main.py:1533`). `--no-tweaks` (wsmr's pre-existing long name) is kept as `-t`'s long form. |
| `-g` / `-G` (graphical-target warn/abort) | ✓ | ✓ | Implemented this phase; skipped entirely for `-o`/`-n`, matching upstream. |
| `COMPOSITOR [ARGS...]` | ✓ | ✓ | |

## `stop`

| Flag | uwsm | wsmr | Notes |
|---|---|---|---|
| `-n` | ✓ | ✓ | |
| `-r [marks]` | ✓, comma-separated compositor id / `tweaks` / `generic` | ✓ | Mark filtering implemented this phase (`session::stop::parse_marks` + `units::plan::mark_of`). `generic` matches nothing in wsmr: upstream's `generic` mark covers its shipped-static unit graph, which wsmr generates but — unlike upstream — never auto-removes (see `docs/coexistence.md`; the static graph is byte-identical, shared infrastructure with uwsm, so content alone can't prove which tool's copy it is). |
| `-U` | ✓ | ✓ | Same fix as `start -U`. |

## `finalize`

Matches exactly: positional `VAR_NAME...`, plus `$UWSM_FINALIZE_VARNAMES`.

## `app`

| Flag | uwsm | wsmr | Notes |
|---|---|---|---|
| `-s` / `-t` / `-T` / `-a` / `-u` / `-d` | ✓ | ✓ | Already matched. |
| `-p Property=value` (repeatable) | ✓ | ✓ | Fixed this phase — wsmr previously only had `--unit-property` (kept as an alias). |
| `-S {out,err,both}` | ✓, value required | ✓ | Fixed this phase — wsmr previously made `--silent` bare-flag-able (defaulting to "both"), which upstream doesn't support. |
| Raw terminal-option passthrough between `-T` and `--` | Upstream pre-scans `sys.argv` for unrecognized flags between `-T` and `--` and forwards them to the terminal emulator (`main.py:1979-2017`) | Not implemented | wsmr instead exposes a fixed set of terminal options (`--app-id`, `--title`, `--dir`, `--hold`) through `TermOpts`/`TerminalArg*` desktop-entry keys. Deliberately not replicating upstream's raw-argv pre-parse: it's a significant, upstream-CLI-specific mechanism: to change without duplicating undue risk in a differently-shaped parser (clap derive vs. hand-rolled pre-scan). Flagged here as a real behavioral gap, not silently dropped. |

## `check is-active [WM]`

Fixed this phase (P2-01) — `WM` was parsed but ignored. Now ports `is_active`'s
full selector logic (`main.py:1194`) exactly, including the "generic" active
set (`graphical-session-pre.target`, `wayland-session-pre@*.target`,
`graphical-session.target`, `wayland-session@*.target`, `wayland-wm@*.service`)
used both for the no-selector case and for `start`'s own double-start refusal
— previously wsmr's refusal only checked `wayland-wm@*.service` and
`graphical-session.target`, missing the mid-startup `*-pre@` window. Verified
live: `wsmr check is-active` (generic) reports `active`, `check is-active
hyprland` reports `inactive`, `check is-active hyprland.desktop` reports
`active`, against the same real session.

## `check may-start`

Already matched upstream's flags and `-g`'s "0 or less disables" semantics
before this phase; no changes needed.

## `aux`

`prepare-env`/`exec`/`waitpid`/`waitenv`/`app-daemon`/`cleanup-env` all match
upstream's subcommand shapes. One minor, low-risk divergence not fixed this
phase: wsmr's `aux exec`/`aux readiness` accept `-D`/`-N`/`-C`/`-e` (shared
`AuxIdArgs`), which upstream's `exec` parser doesn't define at all (only
`prepare-env` gets `wm_meta`'s parent, per `main.py:2226` vs `:2241`).
Harmless in practice — wsmr's own generated units never invoke `aux exec`
with those flags — but an external caller could pass them where upstream
would reject them.

## Compositor id (`CompGlobals.id`)

Fixed this phase — wsmr's desktop-entry resolution stripped the `.desktop`
suffix from the compositor id; upstream never does (`CompGlobals.id` is set
to the raw main-argument's basename *before* entry resolution even runs,
`main.py:3961`). Confirmed both by reading upstream and by observing a real
running uwsm session's unit name (`wayland-wm@hyprland.desktop.service`).

## Known, deliberately deferred divergences

Discovered while reading upstream for this phase but out of P2's explicit
scope (not named in `fix-plan.md`'s P2 findings) — noted here rather than
silently left unmentioned:

- **Static-unit deployment model.** Upstream ships its 13 base graph units
  as vendor package files under `/usr/lib/systemd/user/` (confirmed via
  `pacman -Ql uwsm` on the dev host) and never generates them at runtime;
  `generate_dropins`/`update_unit` only ever touch drop-ins. wsmr, having no
  system package of its own yet, generates the full static graph into the
  rung directory at every `start` (Phase 0's `templates::GRAPH`). This is a
  reasonable, already-load-bearing adaptation (Phase 0's ownership manifest
  and the choice to never auto-delete the graph both depend on it) — noted
  here as the underlying reason for those choices, not a new problem.
- **Other-rung cleanup on every start.** Upstream removes *all* marked units
  from the rung `start` *wasn't* given (`main.py:4803`) on every invocation.
  wsmr does not. Deferred: meaningfully larger surface (a second directory's
  ownership-safe removal on every start) than a documentation-only fix, and
  not named in `fix-plan.md`'s P2 scope.
- **`wayland-wm@.service`'s `TimeoutStartSec` vs. `$UWSM_WAIT_VARNAMES_TIMEOUT`.**
  Upstream syncs a `50_timeout.conf` drop-in so the unit's own startup
  timeout matches the configured wait timeout (`generate_dropins`,
  `main.py:1397-1424`); wsmr's `session::wait::wait_timeout` already reads
  the same env var but nothing updates the generated unit's hardcoded
  `TimeoutStartSec=30`. A real gap if someone raises the wait timeout past
  30s, since the compositor service could be killed by systemd before
  `waitenv`'s own (longer) wait matters. Deferred: not named in `fix-plan.md`.
- **`-v`/`--version` short flag.** Upstream uses `-v`; clap's built-in
  version flag defaults to `-V`. Not fixed — very low value, and `-v` would
  need reconciling against `check is-active -v`/`check may-start -v`'s own
  meaning at the top level.
