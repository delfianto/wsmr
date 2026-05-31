//! wsmr — Wayland Session Manager in Rust, a functional port of `uwsm`.
//!
//! Library crate holding the reusable pieces; the `wsmr` binary (`src/main.rs`)
//! is a thin dispatcher over these modules.
//!
//! See `docs/uwsm-core-analysis.md` for the porting plan and `REFERENCE.md`
//! (local, not committed) for the upstream Python mechanics.

pub mod cli;
pub mod comp;
pub mod env;
pub mod error;
pub mod filter;
pub mod session;
pub mod sysd;
pub mod units;
pub mod util;
pub mod varnames;
