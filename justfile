# wsmr — baseline + container/coverage extras
#
# Native recipes work on macOS *and* Linux. Container recipes need podman.

bins    := "wsmr"
bin_dir := env_var("HOME") / ".local/bin"
sys_dir := "/usr/local/bin"

set positional-arguments

# List available recipes
default:
    @just --list

# Build release binaries
build:
    cargo build --release

# Debug build
build-debug:
    cargo build

# Release build tuned for THIS machine's CPU — fastest, but NOT portable
build-native:
    RUSTFLAGS="-C target-cpu=native" cargo build --release

# Run the binary, e.g. `just run start sway` (Linux only at runtime)
run *args:
    cargo run --release -- "$@"

# Unit/doc tests, native
test *args:
    cargo test "$@"

# Auto-format the tree
fmt:
    cargo fmt --all

# Check formatting (CI gate)
fmt-check:
    cargo fmt --all -- --check

# Lint — warnings denied (CI gate)
lint:
    cargo clippy --all-targets --all-features -- -D warnings

# Full local gate, mirrors CI (fmt + clippy + tests)
check: fmt-check lint test

# Compress every release binary with upx (skips a binary if already packed)
compress: build
    #!/usr/bin/env bash
    set -euo pipefail
    if ! command -v upx >/dev/null 2>&1; then
        echo "compress: upx not found in PATH" >&2
        exit 1
    fi
    for b in {{bins}}; do
        path="target/release/$b"
        if [ ! -f "$path" ]; then
            echo "compress: missing $path (is bins= correct?)" >&2
            exit 1
        fi
        upx -t "$path" >/dev/null 2>&1 || upx --best --lzma "$path"
        echo "compressed $path"
    done

# Install into ~/.local/bin (default) or /usr/local/bin (--system, via sudo)
install *flags: compress
    #!/usr/bin/env bash
    set -euo pipefail
    dir="{{bin_dir}}"
    sudo=""
    for f in {{flags}}; do
        case "$f" in
            --system) dir="{{sys_dir}}"; sudo="sudo" ;;
            *) echo "install: unknown flag '$f' (only --system is supported)" >&2; exit 1 ;;
        esac
    done
    for b in {{bins}}; do
        $sudo install -Dm755 "target/release/$b" "$dir/$b"
        echo "installed $dir/$b"
    done

# Remove installed binaries (pass --system for /usr/local/bin via sudo)
uninstall *flags:
    #!/usr/bin/env bash
    set -euo pipefail
    dir="{{bin_dir}}"
    sudo=""
    for f in {{flags}}; do
        case "$f" in
            --system) dir="{{sys_dir}}"; sudo="sudo" ;;
            *) echo "uninstall: unknown flag '$f' (only --system is supported)" >&2; exit 1 ;;
        esac
    done
    for b in {{bins}}; do
        $sudo rm -f "$dir/$b"
        echo "removed $dir/$b"
    done

# Remove build artifacts + coverage output
clean:
    cargo clean
    rm -rf coverage

# ---------------------------------------------------------------------------
# Specials — Linux container (podman) + coverage
# ---------------------------------------------------------------------------

# Fast type-check (inner loop; not the CI gate — use `check` for that)
typecheck:
    cargo check

# build + cargo test inside the Debian container (Tier A); optional test filter
test-linux *args:
    ./scripts/linux-test.sh "$@"

# cargo build --all-targets + clippy -D warnings on Linux (Tier A)
build-linux:
    ./scripts/linux-build.sh

# full session bootstrap on real systemd (Tier B)
integration:
    ./scripts/linux-integration.sh

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

# everything: native gate + Linux build + integration
ci: check build-linux integration
