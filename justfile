# wsmr task runner — run `just` (or `just --list`) to see everything.
#
# Native recipes work on macOS *and* Linux. The container recipes
# (`test-linux`, `build-linux`, `integration`, `coverage*`) need podman — they
# spin up a Linux / systemd-as-PID-1 container (see CLAUDE.md). On real Linux
# podman runs natively (no VM), so they're just faster there.

set positional-arguments

# list recipes (default)
default:
    @just --list

# ---------------------------------------------------------------- native loop

# fast type-check — the primary inner loop
check:
    cargo check

# debug build
build:
    cargo build

# release build: stripped + heavily optimized (fat LTO, 1 codegen unit, panic=abort)
build-release:
    cargo build --release

# release build tuned for THIS machine's CPU — fastest, but NOT portable
build-native:
    RUSTFLAGS="-C target-cpu=native" cargo build --release

# run the binary, e.g. `just run start sway`  (Linux only at runtime)
run *args:
    cargo run -- "$@"

# unit/doc tests, native (on Linux this also runs the cfg(linux) paths)
test *args:
    cargo test "$@"

# format the tree
fmt:
    cargo fmt --all

# check formatting (CI gate)
fmt-check:
    cargo fmt --all -- --check

# lint with warnings denied (CI gate)
clippy:
    cargo clippy --all-targets --all-features -- -D warnings

# the full native gate, mirroring CI's lint-test job: fmt + clippy + test
lint: fmt-check clippy test

# ------------------------------------------------------- Linux container (podman)

# build + cargo test inside the Debian container (Tier A); optional test filter
test-linux *args:
    ./scripts/linux-test.sh "$@"

# cargo build --all-targets + clippy -D warnings on Linux (Tier A)
build-linux:
    ./scripts/linux-build.sh

# full session bootstrap on real systemd (Tier B)
integration:
    ./scripts/linux-integration.sh

# ----------------------------------------------------------------- coverage

# fast NATIVE coverage subset (no cfg(linux)/integration paths; not the gate)
coverage-unit:
    ./scripts/coverage.sh unit

# authoritative merged coverage with the >= 90% gate (unit + integration, podman)
coverage:
    ./scripts/coverage.sh merged

# merged coverage + an HTML report under coverage/html/
coverage-html:
    WSMR_COV_HTML=1 ./scripts/coverage.sh merged
    @echo "HTML report: coverage/html/index.html"

# ---------------------------------------------------------------------- misc

# everything: native gate + Linux build + integration
ci: lint build-linux integration

# remove build artifacts + coverage output
clean:
    cargo clean
    rm -rf coverage
