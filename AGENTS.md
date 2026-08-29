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

## ⚠️ Critical constraint: macOS dev host, Linux-only target

Development happens on **macOS**, but `wsmr` targets **Linux only** — it relies on
systemd, D-Bus, and Wayland at *runtime*, none of which exist on macOS.

Consequences:
- The crate is **pure Rust** (`zbus`, `nix`, `libc`) — **no C-library linking**, so
  it **builds and unit-tests on macOS**. The only platform-specific syscall (pidfd
  `waitpid`) is `cfg(target_os = "linux")`-gated with a non-Linux stub.
- What macOS **cannot** do is *run* the session logic (no systemd/D-Bus/Wayland).
  `cargo run` / `/run` / `/verify` can't exercise it — don't claim runtime behavior
  was verified unless it ran on Linux.
- **Linux build/test runs in Podman** (see below). Tier A (build + unit tests
  on Linux) and Tier B (systemd-as-PID-1 integration tests) both exist and
  run; Tier B's smoke test asserts the full happy-path session lifecycle as
  hard, unignored checks, so a green run reflects a real pass — it doesn't
  yet cover every failure/recovery scenario worth testing (see
  `docs/fix-plan.md` for exactly which ones). Neither tier runs in CI yet; CI
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
cargo check          # fast type-check (primary loop on macOS)
cargo build          # debug build
cargo test           # unit/doc tests (platform-neutral logic only on macOS)
cargo clippy --all-targets --all-features   # lint
cargo fmt            # format
cargo run -- <args>  # Linux only — needs a live systemd/D-Bus session to do anything
```

## Linux build/test (Podman)

`podman` runs a Linux VM here. Use the wrapper scripts — a bare `cargo test` on
macOS only covers platform-neutral logic, never the Linux paths:

```bash
scripts/linux-test.sh [filter]   # build + cargo test inside a Debian container (Tier A)
scripts/linux-build.sh           # cargo build --all-targets + clippy -D warnings on Linux
scripts/linux-integration.sh     # full session bootstrap on real systemd (Tier B)
# or via the Makefile: make test-unit / test-linux / test-integration / test
```

## Code coverage (cargo-llvm-cov)

```bash
scripts/coverage.sh unit     # fast NATIVE subset (macOS Homebrew LLVM); not the gate
scripts/coverage.sh merged   # authoritative >=90% gate (Podman); the real number
# or: make coverage-unit / make coverage
```

- **Merged is the real number.** A macOS unit-test profile and a Linux
  integration profile can't be merged (different binaries; the `cfg(linux)` pidfd
  path only exists in the Linux build), so the merged number is produced
  end-to-end inside one coverage container (`Containerfile.coverage` =
  systemd-as-PID-1 + Rust): one instrumented build, exercised by BOTH the unit
  tests and the Tier-B integration smoke, reported together
  (`tests/integration/coverage-run.sh`), gated at `--fail-under-lines 90`.
- `scripts/coverage.sh` auto-selects by environment (`uname`, `$CI`,
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
- Source is live bind-mounted; the cargo registry and the Linux `target/` are
  named volumes (`wsmr-cargo-registry`, `wsmr-linux-target`) kept separate from the
  host's macOS `target/`.
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
- Library logic should be testable without a live systemd/D-Bus — isolate
  side-effecting calls behind small traits/wrappers so the port's logic can be
  unit-tested on macOS.
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
