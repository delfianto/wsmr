//! systemd unit handling: string escaping, unit-graph templates, and on-disk
//! generation. See `REFERENCE.md` §8.2 / §14 and `docs/architecture.md`'s
//! unit-graph section.

pub mod escape;
pub mod generate;
pub mod manifest;
pub mod plan;
pub mod templates;
