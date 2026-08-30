//! systemd unit handling: string escaping, unit-graph templates, and on-disk
//! generation. See `docs/architecture/unit-graph.md`.

pub mod escape;
pub mod generate;
pub mod manifest;
pub mod plan;
pub mod templates;
