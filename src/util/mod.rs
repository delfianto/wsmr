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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::with_env;

    #[test]
    fn which_empty_is_none() {
        assert!(which("").is_none());
    }

    #[test]
    fn which_absolute_path() {
        // `/bin/sh` exists and is executable on every unix dev host + container.
        assert_eq!(which("/bin/sh"), Some(PathBuf::from("/bin/sh")));
        assert!(which("/definitely/not/here/xyz").is_none());
    }

    #[test]
    fn which_searches_path() {
        let dir = std::env::temp_dir();
        let bin = dir.join(format!("wsmr-which-{}", std::process::id()));
        std::fs::write(&bin, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let name = bin.file_name().unwrap().to_str().unwrap().to_string();
        with_env(&[("PATH", Some(dir.to_str().unwrap()))], || {
            assert_eq!(which(&name), Some(bin.clone()));
            assert!(which("wsmr-nonexistent-binary-zzz").is_none());
        });
        // a non-executable file on PATH is not resolved
        let plain = dir.join(format!("wsmr-plain-{}", std::process::id()));
        std::fs::write(&plain, "x").unwrap();
        let pname = plain.file_name().unwrap().to_str().unwrap().to_string();
        with_env(&[("PATH", Some(dir.to_str().unwrap()))], || {
            assert!(which(&pname).is_none());
        });
        let _ = std::fs::remove_file(&bin);
        let _ = std::fs::remove_file(&plain);
    }

    #[test]
    fn which_no_path_var() {
        with_env(&[("PATH", None)], || assert!(which("sh").is_none()));
    }

    #[test]
    fn read_fg_vt_errors_when_sysfs_absent() {
        // No /sys/class/tty on macOS dev host (and the file is absent here);
        // exercises the io-error path. On Linux it parses a real ttyN.
        let r = read_fg_vt();
        if cfg!(target_os = "linux") {
            // value or a parse/Resolve error — just don't panic
            let _ = r;
        } else {
            assert!(r.is_err());
        }
    }
}
