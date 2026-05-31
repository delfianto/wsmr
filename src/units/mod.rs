//! systemd unit handling: string escaping, unit-graph templates, and on-disk
//! generation. See `REFERENCE.md` §8.2 / §14 and `docs/uwsm-core-analysis.md`
//! §2.

pub mod escape;
pub mod generate;
pub mod templates;
