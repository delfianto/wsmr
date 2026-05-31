//! Small cross-cutting utilities.

pub mod xdg;

use crate::error::{Error, Result};
use std::path::{Path, PathBuf};

/// Read the foreground VT number from `/sys/class/tty/tty0/active` (e.g. `tty1`
/// → `1`). Linux-only at runtime. Shared by `prepare-env` and `check may-start`.
pub fn read_fg_vt() -> Result<u32> {
    let raw = std::fs::read_to_string("/sys/class/tty/tty0/active")
        .map_err(|e| Error::io("/sys/class/tty/tty0/active", e))?;
    raw.trim()
        .strip_prefix("tty")
        .and_then(|n| n.parse::<u32>().ok())
        .ok_or_else(|| Error::Resolve(format!("unexpected foreground VT: {:?}", raw.trim())))
}

/// Resolve `cmd` to an executable path: if it contains `/`, check it directly;
/// otherwise search `$PATH`. Returns `None` if not found / not executable.
pub fn which(cmd: &str) -> Option<PathBuf> {
    if cmd.is_empty() {
        return None;
    }
    if cmd.contains('/') {
        let p = PathBuf::from(cmd);
        return is_executable(&p).then_some(p);
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(cmd))
        .find(|cand| is_executable(cand))
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.is_file()
        && p.metadata()
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}
