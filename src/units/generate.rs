//! On-disk unit generation: diff-on-write, rung resolution, and writing the
//! graph + drop-ins. Ports `update_unit`/`remove_unit`/`get_unit_path`
//! (`main.py:1275`/`:1340`/`:1117`). `reload` lives in [`crate::sysd`] (M1+).
//! See `REFERENCE.md` §8.2.

use crate::error::{Error, Result};
use crate::units::templates::{self, DropinInput, RenderCtx};
use crate::util::xdg;
use std::path::{Path, PathBuf};

/// Where unit files are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rung {
    /// `$XDG_RUNTIME_DIR/systemd/user` — volatile, per-boot.
    Runtime,
    /// `$XDG_CONFIG_HOME/systemd/user` — persistent.
    Home,
}

/// Outcome of a generation run.
#[derive(Debug, Default)]
pub struct GenOutcome {
    /// Whether any file was created, updated, or removed.
    pub changed: bool,
    /// Relative names written (created/updated).
    pub written: Vec<String>,
    /// Relative names removed.
    pub removed: Vec<String>,
}

impl GenOutcome {
    fn merge(&mut self, other: GenOutcome) {
        self.changed |= other.changed;
        self.written.extend(other.written);
        self.removed.extend(other.removed);
    }
}

/// Resolve the systemd user-unit directory for a rung.
pub fn rung_dir(rung: Rung) -> Result<PathBuf> {
    let base = match rung {
        Rung::Runtime => xdg::runtime_dir()?,
        Rung::Home => xdg::config_home()?,
    };
    Ok(base.join("systemd").join("user"))
}

/// Write `content` to `dir/relname` (creating parent dirs), only if it differs
/// from what's already there. Returns true if created or updated.
pub fn update_unit(dir: &Path, relname: &str, content: &str) -> Result<bool> {
    let path = dir.join(relname);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }
    if let Ok(old) = std::fs::read_to_string(&path)
        && old == content
    {
        return Ok(false);
    }
    std::fs::write(&path, content).map_err(|e| Error::io(&path, e))?;
    Ok(true)
}

/// Remove `dir/relname` if present. Returns true if a file was removed.
pub fn remove_unit(dir: &Path, relname: &str) -> Result<bool> {
    let path = dir.join(relname);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(Error::io(&path, e)),
    }
}

/// Write the full static unit graph (rendered) into `dir`.
pub fn write_graph(dir: &Path, ctx: &RenderCtx) -> Result<GenOutcome> {
    let mut out = GenOutcome::default();
    for unit in templates::GRAPH {
        let body = templates::render(unit.body, ctx);
        if update_unit(dir, unit.name, &body)? {
            out.changed = true;
            out.written.push(unit.name.to_string());
        }
    }
    Ok(out)
}

/// Write (or remove) the per-compositor `50_custom.conf` drop-ins in `dir`.
pub fn write_dropins(dir: &Path, input: &DropinInput) -> Result<GenOutcome> {
    let mut out = GenOutcome::default();
    let preloader = format!(
        "wayland-wm-env@{}.service.d/50_custom.conf",
        input.id_unit_string
    );
    let service = format!(
        "wayland-wm@{}.service.d/50_custom.conf",
        input.id_unit_string
    );

    apply(
        dir,
        &preloader,
        templates::preloader_dropin(input),
        &mut out,
    )?;
    apply(dir, &service, templates::service_dropin(input), &mut out)?;
    Ok(out)
}

fn apply(dir: &Path, relname: &str, text: Option<String>, out: &mut GenOutcome) -> Result<()> {
    match text {
        Some(body) => {
            if update_unit(dir, relname, &body)? {
                out.changed = true;
                out.written.push(relname.to_string());
            }
        }
        None => {
            if remove_unit(dir, relname)? {
                out.changed = true;
                out.removed.push(relname.to_string());
            }
        }
    }
    Ok(())
}

/// Generate the graph + drop-ins for a compositor into `dir`.
pub fn generate(dir: &Path, ctx: &RenderCtx, dropins: &DropinInput) -> Result<GenOutcome> {
    let mut out = write_graph(dir, ctx)?;
    out.merge(write_dropins(dir, dropins)?);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::templates::RenderCtx;
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
                std::env::temp_dir().join(format!("wsmr-test-{}-{}", std::process::id(), nanos));
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

    fn ctx() -> RenderCtx {
        RenderCtx {
            bin_name: "wsmr".into(),
            bin_path: "/usr/bin/wsmr".into(),
            waitpid_bin: "waitpid".into(),
        }
    }

    #[test]
    fn graph_writes_then_idempotent() {
        let td = TempDir::new();
        let first = write_graph(td.path(), &ctx()).unwrap();
        assert!(first.changed);
        assert_eq!(first.written.len(), templates::GRAPH.len());
        // file actually present and rendered
        let wm = std::fs::read_to_string(td.path().join("wayland-wm@.service")).unwrap();
        assert!(wm.contains("ExecStart=/usr/bin/wsmr aux exec -- %I"));
        // second run: no change
        let second = write_graph(td.path(), &ctx()).unwrap();
        assert!(!second.changed);
        assert!(second.written.is_empty());
    }

    #[test]
    fn update_unit_detects_change() {
        let td = TempDir::new();
        assert!(update_unit(td.path(), "a.service", "x\n").unwrap());
        assert!(!update_unit(td.path(), "a.service", "x\n").unwrap());
        assert!(update_unit(td.path(), "a.service", "y\n").unwrap());
    }

    #[test]
    fn dropins_written_in_subdir_and_removable() {
        let td = TempDir::new();
        let input = DropinInput {
            id: "sway".into(),
            id_unit_string: "sway".into(),
            bin_path: "/usr/bin/wsmr".into(),
            bin_name: "sway".into(),
            desktop_names: vec!["sway".into()],
            cmdline: vec!["/usr/bin/sway".into()],
            ..Default::default()
        };
        let out = write_dropins(td.path(), &input).unwrap();
        assert!(out.changed);
        let svc = td.path().join("wayland-wm@sway.service.d/50_custom.conf");
        assert!(svc.exists());

        // a minimal (no-customization) input removes the drop-ins again
        let minimal = DropinInput {
            id: "sway".into(),
            id_unit_string: "sway".into(),
            bin_path: "/usr/bin/wsmr".into(),
            bin_name: "sway".into(),
            desktop_names: vec!["sway".into()],
            cmdline: vec!["sway".into()],
            ..Default::default()
        };
        let out2 = write_dropins(td.path(), &minimal).unwrap();
        assert!(out2.changed);
        assert!(!svc.exists());
    }

    #[test]
    fn remove_missing_is_noop() {
        let td = TempDir::new();
        assert!(!remove_unit(td.path(), "nope.service").unwrap());
    }
}
