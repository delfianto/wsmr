# Development

wsmr is developed and run on Linux. The runtime depends on systemd, logind,
D-Bus, and Wayland; non-Linux development is not supported.

## Local workflow

Use `just` as the main entry point:

```sh
just                    # list recipes
just typecheck          # cargo check
just format --apply     # format the crate
just lint               # clippy with warnings denied
just test               # Rust unit and compatibility tests
just full-gate          # formatting check, lint, and tests
```

Raw Cargo commands also work:

```sh
cargo check
cargo build
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all
```

`just build` is not a generic portable release build. It enables native CPU
instructions and requires UPX. Use `cargo build --release --locked` when you
want a portable release binary.

## Repository layout

| Path | Responsibility |
|---|---|
| `src/cli.rs` | Clap command definitions. |
| `src/session/` | Start, stop, readiness, environment preparation, and cleanup. |
| `src/units/` | Unit templates, planning, ownership, and file generation. |
| `src/env/` | Environment snapshots and delta calculation. |
| `src/app/` | Desktop entries, terminal selection, and application launching. |
| `src/sysd/` | Blocking zbus interfaces for systemd, logind, and D-Bus. |
| `src/util/` | XDG and filesystem helpers. |
| `libexec/` | Embedded POSIX helpers adapted from uwsm. |
| `tests/integration/` | Systemd-container smoke and failure scenarios. |

For control flow, continue with [Architecture](architecture.md).

## Design rules

- Keep side-effect-free decisions separate from systemd, process, and file
  operations so they can be unit-tested.
- Return typed errors from the library. The binary may add context with
  `anyhow`.
- Reserve `panic!`, `unwrap`, and `expect` for genuine invariants.
- Keep unsafe code small and document every block with a `SAFETY` comment.
- Match actual upstream uwsm behavior unless a divergence is documented.

When checking upstream behavior, read a real checkout or installed copy. There
is no `uwsm/` sibling checkout in this repository.

## Commits

Use Conventional Commits. Mark breaking changes with `!`, for example
`feat!:` or `fix!:`.
