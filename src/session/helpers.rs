//! Embedded shell helpers (adapted from uwsm's `uwsm-libexec/`) and their
//! runtime extraction. Per the porting plan we reuse the POSIX scripts verbatim
//! rather than reimplementing the profile/env-file sourcing in Rust.
//!
//! License/attribution for the adapted scripts is to be reconciled (follow-up).

use crate::error::{Error, Result};
use crate::util::xdg;
use std::path::{Path, PathBuf};

const PREPARE_ENV_SH: &str = include_str!("../../libexec/prepare-env.sh");
const SIGNAL_HANDLER_SH: &str = include_str!("../../libexec/signal-handler.sh");

/// Extract a named helper into `$XDG_RUNTIME_DIR/wsmr/libexec/`, mark it
/// executable, and return its path.
pub fn extract(name: &str) -> Result<PathBuf> {
    let body = match name {
        "prepare-env.sh" => PREPARE_ENV_SH,
        "signal-handler.sh" => SIGNAL_HANDLER_SH,
        _ => return Err(Error::InvalidArg(format!("unknown helper script {name:?}"))),
    };
    let dir = xdg::runtime_dir()?.join("wsmr").join("libexec");
    std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
    let path = dir.join(name);
    std::fs::write(&path, body).map_err(|e| Error::io(&path, e))?;
    set_executable(&path)?;
    Ok(path)
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| Error::io(path, e))
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_scripts_present() {
        assert!(PREPARE_ENV_SH.contains("__RANDOM_MARK__"));
        assert!(SIGNAL_HANDLER_SH.contains("systemctl --user"));
    }
}
