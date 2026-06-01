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
> *stub* compositor on real systemd (containerized, and in CI), but it has not
> babysat a daily-driver desktop for months. See [Status & disclaimer](#status--disclaimer).

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

- **Compositor selection.** Your display manager (SDDM here) picks the session;
  wsmr just does the systemd plumbing for the command it's handed.
- Shell plugins/quirks, tweak drop-ins, `fumon`, `ttyautolock`, and the rest of
  uwsm's surface. Not ported.

## How it works (the part you actually care about)

### The unit graph

`start` renders the full graph of systemd **user** units (with the running
binary's path baked in) into the unit rung — `$XDG_RUNTIME_DIR/systemd/user` by
default, `$XDG_CONFIG_HOME/systemd/user` with `-U home` — diff-on-write, so a
re-run is a no-op when nothing changed. Unlike uwsm (which ships most units
statically via its build and only generates the drop-ins), wsmr generates the
**whole** graph at runtime; that keeps it a self-contained binary and means the
units can never drift from the binary that wrote them.

```
graphical-session.target            (the goal; standard systemd target)
└─ wayland-session@<wm>.target
   ├─ wayland-session-pre@.target            pre-session ordering anchor
   ├─ wayland-wm-env@<wm>.service            ExecStart=wsmr aux prepare-env  (the env loader)
   ├─ wayland-wm@<wm>.service                ExecStart=wsmr aux exec -- <wm> (the compositor)
   ├─ wayland-session-waitenv.service        blocks until the env is ready
   ├─ wayland-session-xdg-autostart@.target  pulls in xdg-desktop-autostart
   └─ wayland-session-shutdown.target        OnSuccess/teardown fan-out
wayland-session-bindpid@<pid>.service        binds the session to the launching PID
wayland-session-envelope@<wm>.target         what the anchor process exec's onto
wayland-wm-app-daemon.service                optional `app` fast-path daemon
{app,background,session}-graphical.slice      where launched apps live
```

`BindsTo`/`PropagatesStopTo`/`Conflicts`/`OnSuccess` wiring makes the whole thing
stop as a unit when the compositor exits.

### Session lifecycle

```
wsmr start <wm>
  ├─ generate units + per-compositor 50_custom.conf drop-ins, daemon-reload
  ├─ refuse if a compositor is already active
  ├─ start wayland-session-bindpid@<pid>  (session lifetime ↔ this process)
  ├─ snapshot the login environment to $XDG_RUNTIME_DIR/wsmr/
  └─ exec → systemd-cat → signal-handler.sh → the envelope target   (process is replaced)

# meanwhile, started by the units:
wsmr aux prepare-env   deduce seat/VT/session via logind, run the POSIX env
                       loader, diff pre/post env, push the delta to systemd --user
                       + D-Bus activation env
wsmr aux exec          spawn the readiness watcher, then exec the compositor in
                       the unit's cgroup
wsmr aux readiness     wait for WAYLAND_DISPLAY (+ UWSM_WAIT_VARNAMES) to appear,
                       sync the env delta, then systemd-notify READY=1
→ graphical-session.target is reached

wsmr stop
  └─ stop wayland-wm@<wm> → cascade tears down the session → cleanup-env restores
     the activation environment from the recorded delta
```

The readiness watcher is **spawned, not forked** — `zbus`'s async-io reactor
thread does not survive `fork()`, so a forked watcher's D-Bus connection is dead
and never signals readiness. (This was found the hard way, via the integration
test. It's the kind of bug that only shows up on real systemd.)

### The environment delta

The hard part of a Wayland session is the environment. wsmr snapshots the
activation environment *before* running the shell loader (`prepare-env.sh`,
sourcing your profile etc.), snapshots it *after*, and computes a typed set
delta — honoring uwsm's variable classes (`session_specific`, `always_export`,
`never_export`, `always_unset`, `always_cleanup`, `never_cleanup`) — then
`set`/`unset`s exactly that delta on `systemd --user` and the D-Bus activation
environment. `cleanup-env` on shutdown reverses precisely what was set. This is
all pure set-algebra and is the most thoroughly unit-tested part of the crate.

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
- To build: a Rust toolchain, **edition 2024** (rustc ≥ 1.85; developed on 1.95).
  No system libraries — it's pure Rust.

## Build

```sh
cargo build --release        # or: just build-release
```

The release profile is tuned for execution speed and stripped (fat LTO, one
codegen unit, `panic = "abort"`). For a CPU-tuned, non-portable build:

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

wsmr is developed on **macOS** but only *runs* on **Linux** (no
systemd/D-Bus/Wayland on Darwin). Because it's pure Rust, it builds and
unit-tests on either; the macOS run just skips the Linux-only paths.

```sh
just lint            # fmt + clippy -D warnings + cargo test (the CI gate)
just test            # unit/doc tests (on Linux this also runs the cfg(linux) code)
```

Anything touching a live session runs in Podman (works on macOS via a Linux VM,
natively on Linux):

```sh
just test-linux      # Tier A: build + unit tests in a Debian container
just integration     # Tier B: full session bootstrap on systemd-as-PID-1
just coverage        # merged unit + integration coverage, gated at >= 90% lines
```

See [`CLAUDE.md`](CLAUDE.md) for the container/coverage internals and
[`docs/uwsm-core-analysis.md`](docs/uwsm-core-analysis.md) for the porting spec
(unit graph, env-delta lifecycle, module layout).

## Status & disclaimer

This is an experiment. It reaches into your login session, your `systemd --user`
manager, and your D-Bus activation environment *on purpose*. The lifecycle is
verified against a stub compositor on real systemd (in a container and in CI) —
it is **not** a hardened, daily-driven session manager yet.

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
