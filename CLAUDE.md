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

Development happens on **macOS**, but `wsmr` targets **Linux only** — it depends
on systemd, D-Bus, and Wayland, none of which exist on macOS, and linking
`libsystemd`/`libdbus` will fail here.

Consequences:
- `cargo check` / `cargo build` work **only while the code stays
  platform-neutral**. Once real systemd/D-Bus FFI lands, native macOS builds
  will break — that is expected.
- `cargo run` and the `/run` and `/verify` skills **cannot exercise this app on
  macOS**. Don't claim runtime behavior was verified unless it ran on Linux.
- A Linux execution path (container/VM + `ubuntu-latest` CI) is required before
  any runtime testing. **`podman` is available on this machine.**
- **Deferred (do not build yet — "far ahead"):** integration tests gated on
  detecting `podman`/`docker`, spinning an ephemeral Linux+systemd container to
  run real session-management assertions, skipping cleanly when absent.

## Commands

```bash
cargo check          # fast type-check (primary loop on macOS)
cargo build          # debug build
cargo test           # unit/doc tests (platform-neutral logic only on macOS)
cargo clippy --all-targets --all-features   # lint
cargo fmt            # format
cargo run -- <args>  # Linux only once systemd/D-Bus code exists
```

When checking that Linux-only code compiles without a Linux host, prefer
`cargo check --target x86_64-unknown-linux-gnu` (type-checks; will not fully
link system libs).

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
