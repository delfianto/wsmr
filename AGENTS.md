# Repository guidance

## Project

wsmr is a Rust 2024 port of the core of
[uwsm](https://github.com/Vladimir-csp/uwsm). It manages standalone Wayland
compositor sessions through systemd user units, logind, and D-Bus.

Read these pages according to the task:

- [Architecture](docs/architecture.md) for control flow and design.
- [Commands](docs/commands.md) for the CLI surface.
- [Testing](docs/testing.md) for the test tiers and coverage.
- [Known issues](docs/known-issues.md) for real-hardware findings.
- [Open work](docs/todo.md) for work that is actually still open.

Do not infer project status from old commits or comments when `docs/todo.md`
and the current tests can answer it directly.

## Upstream reference

Match actual uwsm behavior unless a divergence is documented. There is no
`uwsm/` sibling checkout in this repository. When compatibility is in
question, inspect a real checkout or installed copy instead of relying on
memory.

## Platform

Development and runtime are Linux-only. Do not add a non-Linux workflow.

Normal Cargo commands run directly on the host. Podman is used only for a
reproducible build environment and for Tier B, which must boot systemd as PID
1.

The crate uses pure-Rust zbus and does not link to libsystemd or libdbus. The
Linux pidfd implementation has a non-Linux stub for tidy conditional
compilation; that stub does not imply platform support.

## Commands

Prefer `just` recipes:

```sh
just typecheck
just format --check-only
just lint
just test
just full-gate
just test-linux
just build-linux
just integration
just coverage
```

`just build` enables `target-cpu=native` and requires UPX. For a portable
release build, use:

```sh
cargo build --release --locked
```

Raw development commands remain valid:

```sh
cargo check
cargo build
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all
```

## Test model

- Native `cargo test` covers library logic and the captured uwsm unit
  compatibility test.
- Tier A runs build, test, and lint in a pinned Debian container.
- Tier B boots systemd and runs the happy-path session smoke test.
- `scripts/linux-integration-failures.sh` runs six failure scenarios on fresh
  container boots.
- Merged coverage combines native tests and Tier B and enforces 90% lines.

Keep side-effecting systemd, D-Bus, process, and filesystem operations behind
small boundaries. New logic should be testable without a live desktop session
whenever possible.

Environment-driven tests must use `testutil::with_env`; process environment is
global, and mutation is unsafe in Rust 2024.

## Rust conventions

- Edition 2024; rustc 1.98 or newer, as declared in `Cargo.toml`.
- Use `Result` for fallible work.
- Use the typed library `Error` from `src/error.rs`; use `anyhow` at the binary
  boundary.
- Reserve `panic!`, `unwrap`, and `expect` for genuine invariants.
- Keep unsafe and FFI small. Every unsafe block needs a `SAFETY` comment.
- The D-Bus layer is synchronous and uses `zbus::blocking`; do not introduce
  an async runtime without a design reason.
- Desktop-entry and XDG handling is intentionally hand-written for the subset
  wsmr needs.

Globally installed Rust skills may be used for idioms, error design,
concurrency, lifecycle, refactoring, and unsafe review when they are available
to the current agent.

## Commits

Use Conventional Commits. Add `!` for a breaking change, such as `feat!:` or
`fix!:`.
