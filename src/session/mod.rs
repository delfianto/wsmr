//! Session orchestration: environment preparation, finalization, cleanup, the
//! readiness wait/exec machinery, and the `start` exec-chain.
//!
//! Most of this only does real work on **Linux** (D-Bus, fork/exec, systemd);
//! it is written to compile everywhere, with non-Linux fallbacks where a syscall
//! is Linux-only. **Runtime-unverified until the integration phase.**
//! See `REFERENCE.md` §3/§4/§5/§9.

pub mod cleanup;
pub mod exec;
pub mod finalize;
pub mod helpers;
pub mod prepare;
pub mod start;
pub mod stop;
pub mod wait;

use crate::error::Result;
use std::path::PathBuf;

/// Program name used for runtime paths and identifiers.
pub const BIN_NAME: &str = "wsmr";

/// Path to a file under `$XDG_RUNTIME_DIR/wsmr/`.
pub(crate) fn runtime_path(name: &str) -> Result<PathBuf> {
    Ok(crate::util::xdg::runtime_dir()?.join(BIN_NAME).join(name))
}
