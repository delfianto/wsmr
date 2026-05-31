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
scripts/linux-test.sh [filter]   # build + cargo test inside a Debian container
scripts/linux-build.sh           # cargo build --all-targets + clippy -D warnings on Linux
```

- `Containerfile`: Rust + `build-essential` only (NO libdbus/libsystemd — wsmr is
  pure-Rust `zbus` and shells out to `systemctl`/`systemd-notify`).
- Source is live bind-mounted; the cargo registry and the Linux `target/` are
  named volumes (`wsmr-cargo-registry`, `wsmr-linux-target`) kept separate from the
  host's macOS `target/`.
- **Tier B (planned):** a `podman run --systemd=always` PID-1 container for real
  integration tests of prepare-env/exec/start against live systemd + D-Bus.

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
