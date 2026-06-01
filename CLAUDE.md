# CLAUDE.md

Guidance for working in this repository.

## What this is

`wsmr` ("Wayland Session Manager in Rust") is a **Rust 2024** port of
[`uwsm`](https://github.com/Vladimir-csp/uwsm) (Universal Wayland Session
Manager). It sets up the environment and manages standalone Wayland compositor
sessions, offloading session/XDG-autostart/D-Bus-activation-environment handling
to **systemd**.

The upstream Python implementation lives in [`uwsm/`](uwsm/) and is the
**reference to port from** (it has its own `.git`; it is untracked here, kept as
read-only source material — do not edit it).

**Status:** scaffolding stage. Root is a freshly `cargo init`'d binary crate
(`src/main.rs` is still hello-world). No port code written yet.

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
- **Linux build/test runs in Podman** (see below). Tier A (build + unit tests on
  Linux) works today; Tier B (systemd-as-PID-1 integration tests) is next.

## Commands

Prefer **`just <recipe>`** as the entry point (`justfile`; run `just` for the
full list — `build`/`build-release`/`run`/`test`/`lint`/`coverage`/`integration`…).
`build-release` is stripped + heavily optimized (fat LTO, 1 codegen unit,
panic=abort); `build-native` adds `-C target-cpu=native` (fastest, non-portable).
The raw equivalents:

```bash
cargo check          # fast type-check (primary loop on macOS)
cargo build          # debug build
cargo test           # unit/doc tests (platform-neutral logic only on macOS)
cargo clippy --all-targets --all-features   # lint
cargo fmt            # format
cargo run -- <args>  # Linux only once systemd/D-Bus code exists
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

- **Edition 2024**, current stable toolchain (≥1.85 required; 1.95 installed).
- Library logic should be testable without a live systemd/D-Bus — isolate
  side-effecting calls behind small traits/wrappers so the port's logic can be
  unit-tested on macOS.
- Error handling: `Result` everywhere; reserve `panic!`/`unwrap`/`expect` for
  genuine invariants. (`thiserror` for typed library errors, `anyhow` at the
  binary boundary are the likely choices.)
- Keep `unsafe`/FFI minimal and isolated, each block with a `// SAFETY:` note.
- Match upstream behavior unless there's a documented reason to diverge; note
  intentional divergences.

## Likely crates (not yet added — decide before adding)

- CLI: `clap` (derive) to mirror the argparse subcommand tree
- D-Bus: `zbus` (async, pure-Rust) — pairs with `tokio`
- systemd: `zbus` to talk to systemd's D-Bus API, or `libsystemd`/`sd-notify` FFI
- desktop entries / XDG: `freedesktop-desktop-entry`, `xdg`

## Rust skills available (installed globally)

`zhanghandong/rust-skills` (modules incl. `domain-cli`, `m06-error-handling`,
`m07-concurrency`, `m11-ecosystem`, `m12-lifecycle`, `unsafe-checker`,
`rust-refactor-helper`, LSP navigators), `rust-best-practices` (Apollo),
`rust-async-patterns`. Lean on these for idioms, error/concurrency design, and
FFI safety review.

## Commits

Upstream uses Conventional Commits; a `!` marks breaking changes
(`feat!:`, `fix!:`, `chore!:`). Mirror that style here.
