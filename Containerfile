# Reproducible Linux build/test environment for wsmr — a clean, pinned Debian
# image independent of the dev host's own toolchain, not a way to reach Linux
# in the first place (wsmr is developed and run on Linux only). Its
# dependencies are pure Rust (zbus, nix, libc) — there is NO libdbus/libsystemd
# linking, so this image only needs the Rust toolchain plus a C linker (gcc).
# systemd and D-Bus at *runtime* belong in the Tier-B "systemd as PID 1" image,
# not here.
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates curl build-essential \
    && rm -rf /var/lib/apt/lists/*

# Rust toolchain (stable; edition 2024 requires >= 1.85)
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --default-toolchain stable --profile minimal
ENV PATH="/root/.cargo/bin:${PATH}"
RUN rustup component add clippy rustfmt
ENV CARGO_TERM_COLOR=always

WORKDIR /workspace
