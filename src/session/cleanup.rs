//! `cleanup-env`: scrub session-added variables and restore the pre-session
//! activation environment. Ports `cleanup_env` (`main.py:2922`).
//! See `REFERENCE.md` §6.

use crate::error::Result;
use crate::session::{runtime_path, state};
use crate::sysd::dbus::SessionBus;

/// End the current session generation (see [`state::end_generation`]:
/// restores `env_pre`, unsets `(cleanup_list ∪ always_cleanup) − never_cleanup
/// ∩ systemd − env_pre`, and removes the runtime state files), then removes
/// `env_session.conf` (not part of the locked generation state — just a
/// systemd `EnvironmentFile=` mirror with nothing to restore).
pub fn cleanup_env() -> Result<()> {
    let bus = SessionBus::connect()?;
    state::end_generation(&bus)?;
    let _ = std::fs::remove_file(runtime_path("env_session.conf")?);
    Ok(())
}
