//! On-disk unit generation: rung resolution and applying a validated
//! [`GenerationPlan`]/[`RemovalPlan`] (see [`super::plan`]) to disk. Ownership
//! tracking prevents generation and cleanup from mutating files wsmr cannot
//! verify. See `docs/architecture/generated-files.md`.

use super::plan::{Conflict, GenerationPlan, RemovalPlan};
use crate::error::{Error, Result};
use crate::util::{fsutil, xdg};
use std::path::{Path, PathBuf};

/// Where unit files are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rung {
    /// `$XDG_RUNTIME_DIR/systemd/user` — volatile, per-boot.
    Runtime,
    /// `$XDG_CONFIG_HOME/systemd/user` — persistent.
    Home,
}

/// Outcome of applying a plan.
#[derive(Debug, Default)]
pub struct GenOutcome {
    /// Whether any file was created, updated, or removed.
    pub changed: bool,
    /// Relative names written (created/updated).
    pub written: Vec<String>,
    /// Relative names removed.
    pub removed: Vec<String>,
}

/// Resolve the systemd user-unit directory for a rung.
pub fn rung_dir(rung: Rung) -> Result<PathBuf> {
    let base = match rung {
        Rung::Runtime => xdg::runtime_dir()?,
        Rung::Home => xdg::config_home()?,
    };
    Ok(base.join("systemd").join("user"))
}

/// Build the refusal error for a plan carrying conflicts. Never call this
/// with an empty conflict list.
pub fn conflict_error(dir: &Path, conflicts: &[Conflict]) -> Error {
    let mut msg = format!(
        "refusing to touch {} path(s) in {} that are not verifiably owned by wsmr:\n",
        conflicts.len(),
        dir.display()
    );
    for c in conflicts {
        msg.push_str(&format!("  {} \u{2014} {}\n", c.relname, c.reason));
    }
    msg.push_str(
        "Nothing was written. If this belongs to another session manager (e.g. uwsm), \
         leave it running; otherwise inspect and remove it manually before retrying.",
    );
    Error::GenerationConflict(msg)
}

/// One already-applied step, kept so a later failure in the same batch can
/// be rolled back by restoring what was there before.
struct Applied {
    relname: String,
    previous: Option<String>,
}

fn rollback(dir: &Path, applied: &[Applied]) {
    for a in applied.iter().rev() {
        match &a.previous {
            Some(content) => {
                let _ = fsutil::atomic_write(dir, &a.relname, content);
            }
            None => {
                let _ = std::fs::remove_file(dir.join(&a.relname));
            }
        }
    }
}

/// Remove `parent` if it's an empty subdirectory of `dir` (never `dir`
/// itself, and never recursively — only a directory that is already empty).
fn remove_empty_parent(dir: &Path, removed_path: &Path) {
    if let Some(parent) = removed_path.parent()
        && parent != dir
    {
        let _ = std::fs::remove_dir(parent);
    }
}

/// Apply a [`GenerationPlan`] previously built by [`super::plan::plan_generate`].
///
/// Refuses outright (writing nothing) if the plan still carries conflicts —
/// callers should normally have already checked and reported those, but this
/// is the last line of defense. On a mid-batch write failure, already-applied
/// steps in this call are rolled back to their prior content before the error
/// is returned, so a partial failure never leaves a mixed old/new graph.
pub fn apply_generate(dir: &Path, plan: GenerationPlan) -> Result<GenOutcome> {
    if !plan.conflicts.is_empty() {
        return Err(conflict_error(dir, &plan.conflicts));
    }

    let mut manifest = plan.manifest;
    let mut applied: Vec<Applied> = Vec::new();
    let mut out = GenOutcome::default();

    for w in &plan.writes {
        let previous = std::fs::read_to_string(dir.join(&w.relname)).ok();
        if let Err(e) = fsutil::atomic_write(dir, &w.relname, &w.content) {
            rollback(dir, &applied);
            return Err(e);
        }
        applied.push(Applied {
            relname: w.relname.clone(),
            previous,
        });
        manifest.record(&w.relname, &w.content);
        out.changed = true;
        out.written.push(w.relname.clone());
    }

    for r in &plan.removes {
        let path = dir.join(&r.relname);
        let previous = std::fs::read_to_string(&path).ok();
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                rollback(dir, &applied);
                return Err(Error::io(&path, e));
            }
        }
        applied.push(Applied {
            relname: r.relname.clone(),
            previous,
        });
        manifest.forget(&r.relname);
        remove_empty_parent(dir, &path);
        out.changed = true;
        out.removed.push(r.relname.clone());
    }

    if out.changed {
        manifest.save(dir)?;
    }
    Ok(out)
}

/// Apply a [`RemovalPlan`] previously built by [`super::plan::plan_remove_all`].
///
/// Only ever removes manifest-owned, content-verified per-compositor
/// drop-ins (see [`super::plan::plan_remove_all`] docs on why the static
/// graph is excluded). Entries the plan marked as skipped are left on disk
/// and dropped from the manifest so a stale/drifted entry can't keep coming
/// back as a false conflict.
pub fn apply_removal(dir: &Path, plan: RemovalPlan) -> Result<GenOutcome> {
    let mut manifest = plan.manifest;
    let mut applied: Vec<Applied> = Vec::new();
    let mut out = GenOutcome::default();

    for r in &plan.removes {
        let path = dir.join(&r.relname);
        let previous = std::fs::read_to_string(&path).ok();
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                rollback(dir, &applied);
                return Err(Error::io(&path, e));
            }
        }
        applied.push(Applied {
            relname: r.relname.clone(),
            previous,
        });
        manifest.forget(&r.relname);
        remove_empty_parent(dir, &path);
        out.changed = true;
        out.removed.push(r.relname.clone());
    }

    for skipped in &plan.skipped {
        manifest.forget(&skipped.relname);
    }

    if out.changed || !plan.skipped.is_empty() {
        manifest.save(dir)?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::manifest::Manifest;
    use crate::units::plan::{plan_generate, plan_remove_all};
    use crate::units::templates::{self, DropinInput, RenderCtx};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> TempDir {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let p = std::env::temp_dir().join(format!(
                "wsmr-generate-{}-{}",
                std::process::id(),
                nanos
            ));
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

    fn dropin_input() -> DropinInput {
        DropinInput {
            id: "sway".into(),
            id_unit_string: "sway".into(),
            bin_path: "/usr/bin/wsmr".into(),
            bin_name: "sway".into(),
            desktop_names: vec!["sway".into()],
            cmdline: vec!["/usr/bin/sway".into()],
            ..Default::default()
        }
    }

    #[test]
    fn rung_dir_resolves_per_rung() {
        use crate::testutil::with_env;
        with_env(&[("XDG_RUNTIME_DIR", Some("/run/user/1000"))], || {
            assert_eq!(
                rung_dir(Rung::Runtime).unwrap(),
                PathBuf::from("/run/user/1000/systemd/user")
            );
        });
        with_env(&[("XDG_CONFIG_HOME", Some("/home/u/.config"))], || {
            assert_eq!(
                rung_dir(Rung::Home).unwrap(),
                PathBuf::from("/home/u/.config/systemd/user")
            );
        });
    }

    #[test]
    fn generate_then_regenerate_is_idempotent() {
        let td = TempDir::new();
        let plan = plan_generate(td.path(), &ctx(), &dropin_input(), true, false).unwrap();
        let out = apply_generate(td.path(), plan).unwrap();
        assert!(out.changed);
        assert!(td.path().join("wayland-wm@.service").exists());
        assert!(
            td.path()
                .join("wayland-wm@sway.service.d/50_custom.conf")
                .exists()
        );

        let plan2 = plan_generate(td.path(), &ctx(), &dropin_input(), true, false).unwrap();
        assert!(plan2.is_empty());
        let out2 = apply_generate(td.path(), plan2).unwrap();
        assert!(!out2.changed);
    }

    #[test]
    fn manifest_records_only_what_was_written() {
        let td = TempDir::new();
        let plan = plan_generate(td.path(), &ctx(), &dropin_input(), true, false).unwrap();
        apply_generate(td.path(), plan).unwrap();

        let manifest = Manifest::load(td.path()).unwrap();
        let wm = std::fs::read_to_string(td.path().join("wayland-wm@.service")).unwrap();
        assert!(manifest.verify("wayland-wm@.service", &wm));
    }

    #[test]
    fn conflicting_plan_is_refused_and_writes_nothing() {
        let td = TempDir::new();
        std::fs::write(td.path().join("wayland-wm@.service"), "foreign\n").unwrap();

        let plan = plan_generate(td.path(), &ctx(), &dropin_input(), true, false).unwrap();
        assert!(!plan.conflicts.is_empty());
        let err = apply_generate(td.path(), plan).unwrap_err();
        assert!(err.to_string().contains("wayland-wm@.service"));
        // nothing else got written either
        assert!(!td.path().join("wayland-wm@sway.service.d").exists());
        assert_eq!(
            std::fs::read_to_string(td.path().join("wayland-wm@.service")).unwrap(),
            "foreign\n"
        );
    }

    #[test]
    fn remove_all_leaves_graph_units_and_removes_owned_dropins() {
        let td = TempDir::new();
        let plan = plan_generate(td.path(), &ctx(), &dropin_input(), true, false).unwrap();
        apply_generate(td.path(), plan).unwrap();
        assert!(td.path().join("wayland-wm@.service").exists());

        let removal = plan_remove_all(td.path(), None).unwrap();
        let out = apply_removal(td.path(), removal).unwrap();
        assert!(out.changed);
        // static graph survives
        assert!(td.path().join("wayland-wm@.service").exists());
        // owned per-compositor drop-in (and its now-empty dir) is gone
        assert!(!td.path().join("wayland-wm@sway.service.d").exists());

        // idempotent: running it again changes nothing
        let removal2 = plan_remove_all(td.path(), None).unwrap();
        let out2 = apply_removal(td.path(), removal2).unwrap();
        assert!(!out2.changed);
    }

    #[test]
    fn remove_all_never_deletes_a_sibling_foreign_dropin() {
        let td = TempDir::new();
        let plan = plan_generate(td.path(), &ctx(), &dropin_input(), true, false).unwrap();
        apply_generate(td.path(), plan).unwrap();

        // a foreign sibling drop-in in the same directory
        let sibling = td.path().join("wayland-wm@sway.service.d/10_foreign.conf");
        std::fs::write(&sibling, "not ours\n").unwrap();

        let removal = plan_remove_all(td.path(), None).unwrap();
        apply_removal(td.path(), removal).unwrap();

        // our file is gone, but the directory survives because the sibling
        // is still in it, and the sibling itself is untouched
        assert!(td.path().join("wayland-wm@sway.service.d").exists());
        assert_eq!(std::fs::read_to_string(&sibling).unwrap(), "not ours\n");
    }

    #[test]
    fn remove_missing_dir_is_noop() {
        let missing = std::env::temp_dir().join(format!("wsmr-absent-{}", std::process::id()));
        let plan = plan_remove_all(&missing, None).unwrap();
        assert!(plan.removes.is_empty());
        let out = apply_removal(&missing, plan).unwrap();
        assert!(!out.changed);
    }

    #[test]
    fn a_failed_write_rolls_back_earlier_writes_in_the_same_batch() {
        let td = TempDir::new();
        // Prime the manifest with an entry for the first graph unit so it's
        // eligible to be "updated", then make its parent directory replaced
        // by a file so the *second* planned write fails partway through.
        // Index 5 is `wayland-wm@.service`, which embeds `@BIN_PATH@` (unlike
        // the static targets at the front of the array), so changing
        // `bin_path` below actually changes its rendered content.
        let first = &templates::GRAPH[5];
        let old = templates::render(first.body, &ctx());
        std::fs::write(td.path().join(first.name), &old).unwrap();
        let mut manifest = Manifest::default();
        manifest.record(first.name, &old);
        manifest.save(td.path()).unwrap();

        let new_ctx = RenderCtx {
            bin_name: "wsmr".into(),
            bin_path: "/usr/local/bin/wsmr".into(),
            waitpid_bin: "waitpid".into(),
        };
        let plan = plan_generate(td.path(), &new_ctx, &dropin_input(), true, false).unwrap();
        assert!(plan.conflicts.is_empty());
        // sabotage a later planned write so it cannot possibly succeed: its
        // destination directory is occupied by a plain file.
        let victim_relname = "wayland-wm-env@.service";
        assert!(plan.writes.iter().any(|w| w.relname == victim_relname));
        let victim_path = td.path().join(victim_relname);
        std::fs::create_dir_all(victim_path.parent().unwrap()).unwrap();
        // occupy the destination path itself with a directory, so writing a
        // *file* there fails.
        std::fs::create_dir_all(&victim_path).unwrap();

        let err = apply_generate(td.path(), plan).unwrap_err();
        let _ = err;

        // the first unit's content must have been rolled back to `old`,
        // proving the earlier successful write was undone.
        assert_eq!(
            std::fs::read_to_string(td.path().join(first.name)).unwrap(),
            old
        );
    }
}
