//! Session orchestration: environment preparation, finalization, cleanup, the
//! readiness wait/exec machinery, and the `start` exec-chain.
//!
//! Most of this only does real work on **Linux** (D-Bus, fork/exec, systemd);
//! it is written to compile everywhere, with non-Linux fallbacks where a syscall
//! is Linux-only. **Runtime-unverified until the integration phase.**
//! See `REFERENCE.md` §3/§4/§5/§9.

pub mod check;
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

/// Best-effort desktop notification for a user-facing failure. Detached
/// commands (finalize, app) have no visible stderr, so a notification is the
/// user's signal. Failures to notify are ignored.
pub fn notify_error(summary: &str, body: &str) {
    if let Ok(bus) = crate::sysd::dbus::SessionBus::connect() {
        let _ = bus.notify(summary, body);
    }
}

/// Program name used for runtime paths and identifiers.
pub const BIN_NAME: &str = "wsmr";

/// Path to a file under `$XDG_RUNTIME_DIR/wsmr/`.
pub(crate) fn runtime_path(name: &str) -> Result<PathBuf> {
    Ok(crate::util::xdg::runtime_dir()?.join(BIN_NAME).join(name))
}
