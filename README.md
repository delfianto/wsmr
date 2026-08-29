# wsmr

[![CI](https://github.com/delfianto/wsmr/actions/workflows/ci.yml/badge.svg)](https://github.com/delfianto/wsmr/actions/workflows/ci.yml)

**W**ayland **S**ession **M**anager in **R**ust — a from-scratch Rust 2024 port of
the *core* of [uwsm](https://github.com/Vladimir-csp/uwsm). It wraps a standalone
Wayland compositor (sway, niri, Hyprland, river, labwc, whatever) in a proper
**systemd user session**: the compositor becomes a managed unit, your activation
environment is computed and pushed to `systemd --user` + D-Bus, XDG autostart and
`graphical-session.target` light up, and shutdown tears the whole thing down
cleanly. Apps you launch land in their own scopes/services under the session
slice instead of leaking into the compositor's cgroup.

If none of that sentence meant anything to you, this tool is not for you, and
that's fine. wsmr assumes you already run a compositor straight from a TTY or a
minimal display manager and you understand *why* you'd want systemd to own the
session graph. There is no hand-holding here and no "paste these dotfiles" path.

> **Status: experiment.** The full lifecycle is verified end-to-end against a
> *stub* compositor on real systemd, in a container (`just integration`,
> **not yet run in CI** — see [Development & testing](#development--testing)) —
> it has not babysat a daily-driver desktop for months. See
> [Status & disclaimer](#status--disclaimer) and
> [`docs/fix-plan.md`](docs/fix-plan.md) for exactly what has and hasn't been
> verified.

## Why this exists

uwsm already does this, well, in Python. wsmr is a deliberate re-implementation
of its *core* in Rust: a single static-ish binary, no Python runtime, no C
library linking (`libsystemd`/`libdbus` are never touched — it talks D-Bus over
pure-Rust `zbus` and shells out to `systemctl`/`systemd-notify`). It's also a
study in porting a large, gnarly, side-effect-heavy CLI to typed Rust without
losing fidelity. If you want the mature, full-featured, battle-tested thing today:
**use uwsm.** wsmr is the lean port.

## Scope

In:

- **`start` / `stop`** — bootstrap and tear down the compositor session.
- **`app`** — launch GUI apps as transient systemd scopes/services in the right slice.

Out (by design):

- **Compositor selection.** Your display manager picks the session; wsmr just
  does the systemd plumbing for the command it's handed.
- Shell plugins/quirks, `fumon`, `ttyautolock`, and the rest of uwsm's surface.
  Not ported. (Tweak drop-ins *are* ported — `start -t`/`-T`.)

## How it works (the short version)

`start` renders a full graph of systemd **user** units per compositor
instance (diff-on-write, byte-identical statics to upstream), snapshots and
computes the environment delta the compositor needs, execs into a shell
signal-handler that anchors the session, and lets systemd's own
`BindsTo`/`PropagatesStopTo`/`Conflicts` wiring tear the whole thing down as
one unit when the compositor exits.

![wsmr unit graph](docs/diagrams/unit-graph.svg)

![wsmr session lifecycle](docs/diagrams/session-lifecycle.svg)

The full walkthrough — module layout, the env-delta set-algebra, generation-
scoped locking, and exactly why the readiness watcher is spawned rather than
forked — is in [`docs/architecture.md`](docs/architecture.md). Start there to
understand the code.

### Launching apps

```sh
wsmr app firefox.desktop                 # resolve a desktop entry, expand its Exec
wsmr app -- mpv ~/clip.mkv               # or a bare command
wsmr app -t service -- syncthing         # managed .service instead of a .scope
wsmr app -s b -- some-daemon             # background-graphical.slice
wsmr app -T -- btop                      # run inside the configured terminal
```

`app` resolves the target (desktop-entry id/path or bare exec), expands the
`Exec` field codes (`%f %F %u %U %c %k %i`, including multi-instance fan-out),
optionally wraps it in your terminal (`xdg-terminals.list` or a
`TerminalEmulator` category scan), then hands it to `systemd-run --user` as a
scope (default, dies with you) or a service (managed), in the chosen slice. The
optional `wayland-wm-app-daemon` is a FIFO fast-path so a thin client can launch
apps without paying Rust startup per call.

### Notable divergences from uwsm

- **Spawn, not fork**, for the readiness watcher (zbus reactor; see above).
- **Whole unit graph generated at runtime** rather than partly shipped static.
- **Hand-rolled** desktop-entry parser and `Exec` tokenizer (no pyxdg).
- **Blocking `zbus`**, no async runtime — maps cleanly onto uwsm's synchronous
  polling.
- Compositor **selector dropped** (out of scope).

## Requirements

- Linux with **systemd** (a working `systemd --user` instance), **logind**, and
  **D-Bus** — i.e. a normal modern desktop Linux. wsmr orchestrates these; it does
  not replace them.
- A Wayland compositor you invoke by command.
- To build: a Rust toolchain, **edition 2024**, rustc ≥ **1.98.0** (the pinned
  `rust-version` in `Cargo.toml`; enforced by CI's dedicated MSRV job).
  No system libraries — it's pure Rust.

## Build

```sh
cargo build --release        # or: just build-release
```

The release profile is tuned for execution speed and stripped (thin LTO, one
codegen unit, `panic = "abort"`; thin rather than fat — fat LTO's build-time
and `target/` size cost wasn't worth it for a crate this size). For a
CPU-tuned, non-portable build:

```sh
just build-native            # adds -C target-cpu=native
```

Drop the resulting `target/release/wsmr` wherever you keep local binaries
(`~/.local/bin`, `/usr/local/bin`, …). No install step, no units to install — the
binary writes its own.

## Use

From a TTY login shell:

```sh
wsmr check may-start && exec wsmr start sway
```

From a display manager, point a `wayland-sessions` entry at it:

```ini
# /usr/local/share/wayland-sessions/sway-wsmr.desktop
[Desktop Entry]
Name=Sway (wsmr)
Exec=wsmr start /usr/bin/sway
Type=Application
```

Useful flags: `start -o` (only generate units, then exit — inspect them before
committing), `start -n` (dry run), `start -D name1:name2` (set
`XDG_CURRENT_DESKTOP`), `stop -r` (also remove generated units). A few env knobs
are honored: `UWSM_APP_UNIT_TYPE`, `UWSM_WAIT_VARNAMES[_TIMEOUT|_SETTLETIME]`.

The `aux *` subcommands (`prepare-env`, `exec`, `readiness`, `waitenv`,
`waitpid`, `cleanup-env`, `app-daemon`) are **internal** — they're invoked by the
generated units. Don't call them by hand unless you're debugging.

## Development & testing

wsmr is developed **and** run on **Linux only** — there's no other supported
dev environment. `cargo`/`just` commands work directly on the host, including
the `cfg(target_os = "linux")` code paths (no container or VM needed just to
reach them).

```sh
just lint            # clippy -D warnings only
just test            # unit/doc tests, including the cfg(linux) code
just full-gate       # format --check-only + lint + test — mirrors what CI actually runs
```

**What CI (`ci.yml`) actually runs, per push/PR:** `just format --check-only`,
`just lint`, `cargo build --all-targets --verbose`, `just test`, plus a
separate MSRV job that repeats build+test pinned to the exact
`rust-version` in `Cargo.toml`. That's it — no coverage gate, no Tier B, no
integration matrix.

Anything touching a live session runs in Podman — not to reach Linux (the
host already is Linux), but because Tier B boots systemd as PID 1, which
needs a container's isolation regardless of host setup. **Local-only** —
none of the following run in CI today:

```sh
just test-linux      # Tier A: build + unit tests in a Debian container
just integration     # Tier B: full session bootstrap on systemd-as-PID-1 —
                      # asserts the happy-path lifecycle as hard, unignored
                      # checks; not every failure/recovery scenario is covered yet
just coverage        # merged unit + integration coverage; >= 90% lines is the
                      # authoritative *local* gate — not enforced by CI
```

See [`CLAUDE.md`](CLAUDE.md) for the container/coverage internals, and
[`docs/README.md`](docs/README.md) for the full documentation index —
architecture, known real-world issues, CLI compatibility, coexistence with
uwsm, the upstream porting reference, and the live fix-plan tracker.

## Status & disclaimer

This is an experiment. It reaches into your login session, your `systemd --user`
manager, and your D-Bus activation environment *on purpose*. The lifecycle is
verified against a stub compositor on real systemd, run locally in a
container (`just integration`) — **not currently run in CI**. Unit tests
(`cargo test`) and lint/format *do* run in CI on every push/PR. None of this
adds up to a hardened, daily-driven session manager yet.

It has also had a first, partial real-hardware pass — real Hyprland, real
monitors, a real disposable user — which found three real, non-wsmr bugs
(a kmscon/compositor seat-ownership conflict, an `xdg-desktop-portal-hyprland`
crash on teardown, and a Hyprland environment-restoration gap) and
cross-validated all three against real upstream uwsm to confirm they aren't
wsmr-specific. **That pass only ever used Hyprland.** No other compositor
(niri, sway, river, labwc, ...) has been run through wsmr at all — treat
those as completely untested. See
[`docs/known-issues.md`](docs/known-issues.md) for the full detail and
[`docs/fix-plan.md`](docs/fix-plan.md)'s Phase 7 for exactly what is and
isn't covered.

If you run it on your actual machine and your session faceplants, your autostart
turns to confetti, you get dumped back to a TTY, or your toaster gains sentience
and walks out — that's on you. There is **no warranty** (see [`LICENSE`](LICENSE),
the part in all caps). You clearly know how to recover a Linux session from a
console; that assumption is the price of admission.

## Credits & license

wsmr is MIT-licensed ([`LICENSE`](LICENSE)). It is a port of, and owes everything
to, **[uwsm](https://github.com/Vladimir-csp/uwsm)** by Vladimir-csp — read that
project for the canonical design and the full feature set. The two bundled POSIX
helpers (`libexec/prepare-env.sh`, `libexec/signal-handler.sh`) are adapted from
uwsm and remain under its MIT copyright; see
[`THIRD-PARTY-LICENSES`](THIRD-PARTY-LICENSES). Not affiliated with or endorsed by
the uwsm project.
