//! wsmr — Wayland Session Manager in Rust, a functional port of `uwsm`.
//!
//! Library crate holding the reusable pieces; the `wsmr` binary (`src/main.rs`)
//! is a thin dispatcher over these modules.
//!
//! See `docs/architecture.md` for the design and focused subsystem guides.

pub mod app;
pub mod cli;
pub mod comp;
pub mod coverage;
pub mod env;
pub mod error;
pub mod filter;
pub mod session;
pub mod sysd;
#[cfg(test)]
pub(crate) mod testutil;
pub mod units;
pub mod util;
pub mod varnames;
