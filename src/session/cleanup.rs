//! Remove session-added variables and restore the pre-session environment.

use crate::error::Result;
use crate::session::{runtime_path, state};
use crate::sysd::dbus::SessionBus;

/// End the current session generation (see [`state::end_generation`]:
/// restores `env_pre`, unsets `(cleanup_list ∪ always_cleanup) − never_cleanup
/// ∩ systemd − env_pre`, and removes the runtime state files), then removes
/// `env_session.conf` (not part of the locked generation state — just a
/// systemd `EnvironmentFile=` mirror with nothing to restore) and the
/// `libexec/` helper scripts [`crate::session::helpers::extract`] wrote for
/// this session (identical content next time, so nothing is lost by removing
/// them — unlike `state.lock`, there's no flock reason to keep them around).
pub fn cleanup_env() -> Result<()> {
    let bus = SessionBus::connect()?;
    state::end_generation(&bus)?;
    let _ = std::fs::remove_file(runtime_path("env_session.conf")?);
    let _ = std::fs::remove_dir_all(runtime_path("libexec")?);
    Ok(())
}
