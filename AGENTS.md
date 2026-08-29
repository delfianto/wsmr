# CLAUDE.md

Guidance for working in this repository.

## What this is

`wsmr` ("Wayland Session Manager in Rust") is a **Rust 2024** port of
[`uwsm`](https://github.com/Vladimir-csp/uwsm) (Universal Wayland Session
Manager). It sets up the environment and manages standalone Wayland compositor
sessions, offloading session/XDG-autostart/D-Bus-activation-environment handling
to **systemd**.

The upstream Python implementation is the **reference to port from**. There is
no `uwsm/` sibling checkout in this repository — when a real uwsm install is
available (e.g. `/usr/share/uwsm/modules/uwsm/main.py` on an Arch/CachyOS box
with the `uwsm` package installed), read its actual source directly rather
than relying on memory; several real bugs in this port were only found that
way. [`docs/architecture.md`](docs/architecture.md) is this port's own
design reference (unit graph, session lifecycle, env-delta machinery).

**Status:** past scaffolding — a substantial, working, unit-tested port
(session bootstrap, environment-delta lifecycle, app launching, CLI surface).
`docs/fix-plan.md` is the live, authoritative tracker of exactly what's
verified vs. still open; don't infer status from this paragraph, which will
drift — check that file.

## Critical constraint: Linux only, for development and runtime alike

`wsmr` is developed **and** run **on Linux only** — it relies on systemd, D-Bus,
and Wayland at runtime, and there is no other supported development
environment. Don't assume a cross-platform dev workflow, and don't add code,
docs, or scripts that accommodate one; that's already accounted for.

Consequences:
- `cargo check`/`build`/`test`/`run` all work directly on the host — no
  container, VM, or remote step is needed to reach a real systemd/D-Bus/
  Wayland session. `cargo run -- <args>` needs a live one (see "What this
  is"), same as it would for any Linux tool.
- The crate is still **pure Rust** (`zbus`, `nix`, `libc`) with **no
  C-library linking** — that's a portability property of the dependency
  choices (see "Crate choices" below), not something the project relies on
  or tests for. The one platform-specific syscall (pidfd `waitpid`) is
  `cfg(target_os = "linux")`-gated with a non-Linux stub purely because it's
  cheap to keep tidy, not because a non-Linux build target is supported.
- **Containers (Podman) are still used**, but only where isolation is
  genuinely needed, not to reach Linux at all: Tier A (`scripts/linux-*.sh`)
  runs build/test/lint inside a clean, pinned Debian image for
  reproducibility independent of whatever's on the dev host; Tier B needs a
  container because it boots **systemd as PID 1**, which you can't (and
  shouldn't) do directly on a running desktop. Tier B's smoke test asserts
  the full happy-path session lifecycle as hard, unignored checks, so a
  green run reflects a real pass — it doesn't yet cover every
  failure/recovery scenario worth testing (see `docs/fix-plan.md` for
  exactly which ones). Neither tier runs in CI yet; CI
  (`.github/workflows/ci.yml`) runs format-check, lint, build, and
  `cargo test` (roughly `just full-gate` plus an explicit build step), plus
  a separate MSRV job pinned to `Cargo.toml`'s `rust-version`.

## Commands

Prefer **`just <recipe>`** as the entry point (`justfile`; run `just` for the
full list — `build`/`build-release`/`run`/`test`/`lint`/`coverage`/`integration`…).
`build-release` is stripped + optimized (thin LTO — not fat; fat's build-time
and `target/` size cost wasn't worth it here, see the "Use thin LTO for
release builds" commit — 1 codegen unit, panic=abort); `build-native` adds
`-C target-cpu=native` (fastest, non-portable).
The raw equivalents:

```bash
cargo check          # fast type-check — the primary loop
cargo build          # debug build
cargo test           # unit/doc tests, including cfg(target_os = "linux") paths
cargo clippy --all-targets --all-features   # lint
cargo fmt            # format
cargo run -- <args>  # needs a live systemd/D-Bus session to do anything
```

## Reproducible build/test (Podman)

Optional, not required to reach Linux (the host already is Linux) — these run
build/test/lint inside a clean, pinned Debian image, independent of whatever
toolchain/libraries happen to be installed on the dev host:

```bash
scripts/linux-test.sh [filter]   # build + cargo test inside a Debian container (Tier A)
scripts/linux-build.sh           # cargo build --all-targets + clippy -D warnings, containerized
scripts/linux-integration.sh     # full session bootstrap on real systemd (Tier B — needs the
                                  # container regardless of host, since it boots systemd as PID 1)
# or via the Makefile: make test-unit / test-linux / test-integration / test
```

## Code coverage (cargo-llvm-cov)

```bash
scripts/coverage.sh unit     # fast native subset, run directly on the host; not the gate
scripts/coverage.sh merged   # authoritative >=90% gate (Podman); the real number
# or: make coverage-unit / make coverage
```

- **Merged is the real number.** `unit` mode only exercises what a unit test
  can reach without a live systemd/D-Bus session — it's a real, useful subset
  (runs fast, right on the host), but it's not the full picture. `merged`
  produces the authoritative one end-to-end inside one coverage container
  (`Containerfile.coverage` = systemd-as-PID-1 + Rust): one instrumented
  build, exercised by BOTH the unit tests and the Tier-B integration smoke,
  reported together (`tests/integration/coverage-run.sh`), gated at
  `--fail-under-lines 90`. This still needs a container even on a Linux host,
  for the same systemd-as-PID-1 isolation reason Tier B always does.
- `scripts/coverage.sh` auto-selects by environment (`$CI`,
  `/run/.containerenv`, podman presence): inside a container → run cargo-llvm-cov
  directly; podman available → merged; else native `unit` with a PARTIAL warning.
- **Pre-exec profile flush:** wsmr ends most processes with `exec()`, which skips
  LLVM's `atexit` profraw write. `crate::coverage::flush_before_exec()` (compiled
  only under `cfg(coverage)`, a no-op otherwise) flushes right before each
  `exec()`; the coverage container also propagates `LLVM_PROFILE_FILE` into the
  user manager's activation env so unit-spawned wsmr processes are instrumented.
- Env-driven unit tests serialize through `testutil::with_env` (env is process-
  global and `set_var` is `unsafe` in 2024).

- `Containerfile`: Rust + `build-essential` only (NO libdbus/libsystemd — wsmr is
  pure-Rust `zbus` and shells out to `systemctl`/`systemd-notify`).
- Source is live bind-mounted; the cargo registry and the container's `target/`
  are named volumes (`wsmr-cargo-registry`, `wsmr-linux-target`) kept separate
  from the host's own `target/` so the two builds never collide.
- **Tier B (`Containerfile.systemd`):** boots systemd as PID 1, starts a user
  manager via linger, and runs `tests/integration/smoke.sh` — drives `wsmr start`
  with a stub compositor and asserts the full lifecycle (generate → prepare-env →
  readiness → `graphical-session.target` → shutdown → cleanup). This is wsmr's
  real runtime verification; the session bootstrap (M3) passes here.

## What's being ported (reference map)

Paths below are upstream uwsm's own repo layout (`uwsm/uwsm/main.py` etc.) —
there is no such tree in *this* repo (see "What this is" above); resolve
these against a real uwsm checkout, or an installed package's module dir
(e.g. `/usr/share/uwsm/modules/uwsm/main.py`), when you need to check them.

Upstream Python (`uwsm/uwsm/`): `main.py` (~5.2k lines, the bulk — CLI + session
logic), `dbus.py` (D-Bus helpers), `misc.py` (utilities), `wrapper.py.in` /
`params.py.in` (build-time templated entrypoints). Shell helpers live in
`uwsm/uwsm-libexec/` (`prepare-env.sh`, `signal-handler.sh`) and `uwsm/scripts/`.

CLI surface to reproduce (from `main.py` argparse):

| Command | Purpose |
|---|---|
| `select` | Pick a compositor (desktop-entry chooser) |
| `start` | Start the compositor session via systemd |
| `stop` | Stop the session |
| `finalize` | Export compositor env vars into the systemd/D-Bus activation environment |
| `app` | Launch an app as a scoped/service unit under the session |
| `check is-active` / `check may-start` | Session state predicates |
| `aux {prepare-env,cleanup-env,exec,waitpid,waitenv,app-daemon}` | Internal helpers (invoked by units) |

## Conventions

- **Edition 2024**, rustc ≥ **1.98.0** (pinned as `rust-version` in
  `Cargo.toml`, enforced by CI's MSRV job).
- Library logic should be testable without a live systemd/D-Bus session —
  isolate side-effecting calls behind small traits/wrappers so the port's
  logic can be unit-tested fast and in isolation, without needing Tier B's
  full systemd-as-PID-1 container for every change.
- Error handling: `Result` everywhere; reserve `panic!`/`unwrap`/`expect` for
  genuine invariants. `thiserror` for the typed library `Error`
  (`src/error.rs`), `anyhow` at the binary boundary (`main.rs`).
- Keep `unsafe`/FFI minimal and isolated, each block with a `// SAFETY:` note.
- Match upstream behavior unless there's a documented reason to diverge; note
  intentional divergences.

## Crate choices (decided)

- CLI: `clap` (derive), mirroring uwsm's argparse subcommand tree —
  `src/cli.rs`. See `docs/architecture.md`'s CLI surface section for the
  subcommand table and known divergences.
- D-Bus/systemd: `zbus`, used in **blocking** mode (`zbus::blocking`), no
  `tokio` — maps cleanly onto uwsm's synchronous polling. Talks directly to
  systemd's D-Bus API (`src/sysd/dbus.rs`); no `libsystemd`/`sd-notify` FFI.
- desktop entries / XDG: **hand-rolled**, not `freedesktop-desktop-entry` or
  `xdg` (`src/app/entry.rs`, `src/app/field.rs`, `src/util/xdg.rs`) — kept
  deliberately minimal to the subset wsmr needs, cross-checked against
  `python-pyxdg`'s real behavior where it matters (locale handling).

## Rust skills available (installed globally)

`zhanghandong/rust-skills` (modules incl. `domain-cli`, `m06-error-handling`,
`m07-concurrency`, `m11-ecosystem`, `m12-lifecycle`, `unsafe-checker`,
`rust-refactor-helper`, LSP navigators), `rust-best-practices` (Apollo),
`rust-async-patterns`. Lean on these for idioms, error/concurrency design, and
FFI safety review.

## Commits

Upstream uses Conventional Commits; a `!` marks breaking changes
(`feat!:`, `fix!:`, `chore!:`). Mirror that style here.
