//! Same-directory temp-file-then-rename helpers, so a reader (systemd
//! scanning the unit directory, a later unit generation, or a concurrent
//! reader of session-state files under `$XDG_RUNTIME_DIR/wsmr/`) never
//! observes a partially written file, and a crash mid-write leaves the
//! previous file intact rather than truncated. Shared by unit generation
//! (`crate::units`) and session environment-state persistence
//! (`crate::env`, `crate::session`).

use crate::error::{Error, Result};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Write `content` to `dir/relname` via a temp file created alongside the
/// destination (so the final `rename` is same-directory and atomic on a
/// POSIX filesystem), creating parent directories as needed.
pub fn atomic_write(dir: &Path, relname: &str, content: &str) -> Result<()> {
    let dest = dir.join(relname);
    let parent = dest.parent().unwrap_or(dir);
    std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;

    let tmp = temp_path(&dest);
    let result = write_temp(&tmp, content);
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
        return result;
    }
    std::fs::rename(&tmp, &dest).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        Error::io(&dest, e)
    })
}

/// Write `content` to `path` atomically (see [`atomic_write`]), for callers
/// that already have a full destination path rather than a `(dir, relname)`
/// pair.
pub fn atomic_write_path(path: &Path, content: &str) -> Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let relname = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    atomic_write(dir, &relname, content)
}

fn write_temp(tmp: &Path, content: &str) -> Result<()> {
    let mut f = std::fs::File::create(tmp).map_err(|e| Error::io(tmp, e))?;
    f.write_all(content.as_bytes())
        .map_err(|e| Error::io(tmp, e))?;
    f.sync_all().map_err(|e| Error::io(tmp, e))
}

/// A same-directory, collision-resistant temp path for `dest`.
fn temp_path(dest: &Path) -> PathBuf {
    let parent = dest.parent().unwrap_or_else(|| Path::new("."));
    let base = dest
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    parent.join(format!(".{base}.wsmr-tmp.{}.{nanos}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> TempDir {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let p =
                std::env::temp_dir().join(format!("wsmr-fsutil-{}-{}", std::process::id(), nanos));
            std::fs::create_dir_all(&p).unwrap();
            TempDir(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn writes_and_overwrites_leaving_no_temp_files() {
        let td = TempDir::new();
        atomic_write(td.path(), "a.service", "one\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(td.path().join("a.service")).unwrap(),
            "one\n"
        );
        atomic_write(td.path(), "a.service", "two\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(td.path().join("a.service")).unwrap(),
            "two\n"
        );
        let leftovers: Vec<_> = std::fs::read_dir(td.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("wsmr-tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp files were not cleaned up");
    }

    #[test]
    fn creates_parent_subdirectory() {
        let td = TempDir::new();
        atomic_write(td.path(), "sub/dir/x.conf", "hi\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(td.path().join("sub/dir/x.conf")).unwrap(),
            "hi\n"
        );
    }

    #[test]
    fn atomic_write_path_matches_dir_relname_form() {
        let td = TempDir::new();
        atomic_write_path(&td.path().join("env_pre"), "A=1\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(td.path().join("env_pre")).unwrap(),
            "A=1\n"
        );
    }
}
