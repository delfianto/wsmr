//! Pure, read-only planning for unit generation and removal.
//!
//! Building a plan only stats and reads existing files plus the ownership
//! [`Manifest`] — it never creates, writes, renames, or deletes anything.
//! That split is what makes `--dry-run` strictly read-only (P0-02) and lets
//! generation validate the *complete* set of intended changes — including
//! refusing to touch anything wsmr doesn't verifiably own — before any file
//! is written (P0-04).

use super::manifest::Manifest;
use super::templates::{self, DropinInput, RenderCtx};
use crate::error::{Error, Result};
use std::path::Path;

/// One file wsmr intends to create or update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedWrite {
    /// Path relative to the rung directory.
    pub relname: String,
    /// Full intended content.
    pub content: String,
}

/// One file wsmr intends to remove.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedRemove {
    /// Path relative to the rung directory.
    pub relname: String,
}

/// A destination wsmr wanted to touch but doesn't verifiably own, so the
/// touch is refused (writes) or skipped (removals).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// Path relative to the rung directory.
    pub relname: String,
    /// Human-readable reason, for diagnostics.
    pub reason: &'static str,
}

/// A pure plan for [`super::generate::apply_generate`].
#[derive(Debug)]
pub struct GenerationPlan {
    /// Files to create or update.
    pub writes: Vec<PlannedWrite>,
    /// Now-unneeded drop-ins to remove (only ones wsmr verifiably owns).
    pub removes: Vec<PlannedRemove>,
    /// Destinations that block the whole plan until resolved.
    pub conflicts: Vec<Conflict>,
    pub(super) manifest: Manifest,
}

impl GenerationPlan {
    /// Whether applying this plan would change anything on disk.
    pub fn is_empty(&self) -> bool {
        self.writes.is_empty() && self.removes.is_empty()
    }
}

/// A pure plan for [`super::generate::apply_removal`].
#[derive(Debug)]
pub struct RemovalPlan {
    /// Files to remove (all manifest-owned and content-verified).
    pub removes: Vec<PlannedRemove>,
    /// Tracked paths that failed ownership verification (drifted/tampered
    /// since wsmr wrote them) — never deleted, only reported.
    pub skipped: Vec<Conflict>,
    pub(super) manifest: Manifest,
}

/// Compute the full generation plan for `dropins` into `dir`: the static
/// graph, the fixed tweak drop-ins (written when `tweaks_enabled`, else
/// removed if wsmr owns them — see `templates::TWEAKS`), and this
/// compositor's `50_custom.conf` drop-ins. Read-only.
pub fn plan_generate(
    dir: &Path,
    ctx: &RenderCtx,
    dropins: &DropinInput,
    tweaks_enabled: bool,
) -> Result<GenerationPlan> {
    let manifest = Manifest::load(dir)?;
    let mut plan = GenerationPlan {
        writes: Vec::new(),
        removes: Vec::new(),
        conflicts: Vec::new(),
        manifest,
    };

    for unit in templates::GRAPH {
        let content = templates::render(unit.body, ctx);
        classify_write(dir, unit.name, content, &mut plan)?;
    }

    for tweak in templates::TWEAKS {
        let wanted = tweaks_enabled.then(|| templates::render(tweak.body, ctx));
        classify_dropin(dir, tweak.name, wanted, &mut plan)?;
    }

    let preloader = format!(
        "wayland-wm-env@{}.service.d/50_custom.conf",
        dropins.id_unit_string
    );
    let service = format!(
        "wayland-wm@{}.service.d/50_custom.conf",
        dropins.id_unit_string
    );
    classify_dropin(
        dir,
        &preloader,
        templates::preloader_dropin(dropins),
        &mut plan,
    )?;
    classify_dropin(dir, &service, templates::service_dropin(dropins), &mut plan)?;

    Ok(plan)
}

fn classify_write(
    dir: &Path,
    relname: &str,
    content: String,
    plan: &mut GenerationPlan,
) -> Result<()> {
    let path = dir.join(relname);
    match std::fs::read_to_string(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            plan.writes.push(PlannedWrite {
                relname: relname.to_string(),
                content,
            });
        }
        Err(e) => return Err(Error::io(&path, e)),
        Ok(existing) if existing == content => {
            // Already exactly right — nothing to do, and nothing to claim:
            // leave ownership as-is (see module docs on shared static units).
        }
        Ok(existing) => {
            if plan.manifest.verify(relname, &existing) {
                plan.writes.push(PlannedWrite {
                    relname: relname.to_string(),
                    content,
                });
            } else {
                plan.conflicts.push(Conflict {
                    relname: relname.to_string(),
                    reason: "existing file is not a wsmr-owned generation (unknown origin, or edited since wsmr wrote it)",
                });
            }
        }
    }
    Ok(())
}

fn classify_dropin(
    dir: &Path,
    relname: &str,
    wanted: Option<String>,
    plan: &mut GenerationPlan,
) -> Result<()> {
    let path = dir.join(relname);
    let existing = match std::fs::read_to_string(&path) {
        Ok(s) => Some(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(Error::io(&path, e)),
    };
    match (existing, wanted) {
        (None, Some(content)) => plan.writes.push(PlannedWrite {
            relname: relname.to_string(),
            content,
        }),
        (None, None) => {}
        (Some(existing), Some(content)) => {
            if existing == content {
                // no-op
            } else if plan.manifest.verify(relname, &existing) {
                plan.writes.push(PlannedWrite {
                    relname: relname.to_string(),
                    content,
                });
            } else {
                plan.conflicts.push(Conflict {
                    relname: relname.to_string(),
                    reason: "existing drop-in is not a wsmr-owned generation (unknown origin, or edited since wsmr wrote it)",
                });
            }
        }
        (Some(existing), None) => {
            if plan.manifest.verify(relname, &existing) {
                plan.removes.push(PlannedRemove {
                    relname: relname.to_string(),
                });
            }
            // Else: a foreign file occupies a path wsmr would otherwise
            // manage. It isn't ours to remove, so it's left alone and this
            // is not reported as a conflict — nothing is being overwritten.
        }
    }
    Ok(())
}

/// Compute a plan removing everything wsmr owns and tracks in `dir`, matching
/// upstream's `-r` mark filter (`main.py:1933`): `marks: None` removes
/// everything removable; `Some(marks)` removes only entries whose mark (a
/// compositor id, or `"tweaks"`) is in the list — see [`mark_of`].
///
/// Only per-compositor `50_custom.conf` drop-ins and the fixed tweak
/// drop-ins are ever removed here. The static graph units
/// (`templates::GRAPH`) are deliberately excluded even if somehow present in
/// the manifest: they are byte-identical, shared infrastructure with uwsm
/// (see `docs/coexistence.md`), so content alone can never distinguish
/// "wsmr's copy" from "uwsm's copy" — removing them is never safe to
/// automate. (Upstream's `"generic"` mark, which covers its shipped-static
/// graph, therefore has nothing to match here.)
pub fn plan_remove_all(dir: &Path, marks: Option<&[String]>) -> Result<RemovalPlan> {
    let manifest = Manifest::load(dir)?;
    let mut plan = RemovalPlan {
        removes: Vec::new(),
        skipped: Vec::new(),
        manifest,
    };

    let owned: Vec<String> = plan
        .manifest
        .tracked()
        .filter(|n| is_removable_dropin(n))
        .filter(|n| match marks {
            None => true,
            Some(ms) => mark_of(n).is_some_and(|m| ms.iter().any(|x| x == m)),
        })
        .map(String::from)
        .collect();

    for relname in owned {
        let path = dir.join(&relname);
        match std::fs::read_to_string(&path) {
            Ok(existing) => {
                if plan.manifest.verify(&relname, &existing) {
                    plan.removes.push(PlannedRemove { relname });
                } else {
                    plan.skipped.push(Conflict {
                        relname,
                        reason: "tracked path no longer matches what wsmr wrote (drifted or tampered) — left in place",
                    });
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Already gone; still forget it so cleanup is idempotent.
                plan.removes.push(PlannedRemove { relname });
            }
            Err(e) => return Err(Error::io(&path, e)),
        }
    }
    Ok(plan)
}

fn is_removable_dropin(relname: &str) -> bool {
    is_tweak(relname) || is_compositor_dropin(relname)
}

fn is_tweak(relname: &str) -> bool {
    templates::TWEAKS.iter().any(|t| t.name == relname)
}

fn is_compositor_dropin(relname: &str) -> bool {
    relname.ends_with(".service.d/50_custom.conf")
        && (relname.starts_with("wayland-wm@") || relname.starts_with("wayland-wm-env@"))
}

/// The `-r` mark a tracked path belongs to: a compositor id for its
/// `50_custom.conf` drop-ins, or `"tweaks"` for the fixed tweak drop-ins.
/// `None` for anything `plan_remove_all` wouldn't remove in the first place.
fn mark_of(relname: &str) -> Option<&str> {
    if is_tweak(relname) {
        return Some("tweaks");
    }
    for prefix in ["wayland-wm-env@", "wayland-wm@"] {
        if let Some(id) = relname
            .strip_prefix(prefix)
            .and_then(|rest| rest.strip_suffix(".service.d/50_custom.conf"))
        {
            return Some(id);
        }
    }
    None
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
                std::env::temp_dir().join(format!("wsmr-plan-{}-{}", std::process::id(), nanos));
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
    fn plan_generate_never_touches_disk() {
        let td = TempDir::new();
        let before: Vec<_> = std::fs::read_dir(td.path()).unwrap().collect();
        assert!(before.is_empty());

        let plan = plan_generate(td.path(), &ctx(), &dropin_input(), true).unwrap();
        assert!(plan.conflicts.is_empty());
        // graph + both dropins (the absolute cmdline in `dropin_input()`
        // triggers both the preloader's `-- %I <path>` override and the
        // service's hardcoded `ExecStart=`).
        assert_eq!(
            plan.writes.len(),
            templates::GRAPH.len() + templates::TWEAKS.len() + 2
        );

        let after: Vec<_> = std::fs::read_dir(td.path()).unwrap().collect();
        assert!(after.is_empty(), "plan_generate must not write anything");
    }

    #[test]
    fn absent_destination_plans_a_write() {
        let td = TempDir::new();
        let plan = plan_generate(td.path(), &ctx(), &dropin_input(), true).unwrap();
        assert!(
            plan.writes
                .iter()
                .any(|w| w.relname == "wayland-wm@.service")
        );
    }

    #[test]
    fn foreign_existing_file_is_a_conflict_not_a_write() {
        let td = TempDir::new();
        std::fs::write(
            td.path().join("wayland-wm@.service"),
            "# hand-written by someone else\n",
        )
        .unwrap();

        let plan = plan_generate(td.path(), &ctx(), &dropin_input(), true).unwrap();
        assert!(
            plan.conflicts
                .iter()
                .any(|c| c.relname == "wayland-wm@.service")
        );
        assert!(
            !plan
                .writes
                .iter()
                .any(|w| w.relname == "wayland-wm@.service")
        );
        // untouched
        assert_eq!(
            std::fs::read_to_string(td.path().join("wayland-wm@.service")).unwrap(),
            "# hand-written by someone else\n"
        );
    }

    #[test]
    fn wsmr_owned_file_may_be_updated() {
        let td = TempDir::new();
        // Index 5 (`wayland-wm@.service`) embeds `@BIN_PATH@`, unlike the
        // static targets at the front of the array, so a `bin_path` change
        // actually changes its rendered content.
        let unit = &templates::GRAPH[5];
        let old_content = templates::render(unit.body, &ctx());
        std::fs::write(td.path().join(unit.name), &old_content).unwrap();
        let mut manifest = Manifest::default();
        manifest.record(unit.name, &old_content);
        manifest.save(td.path()).unwrap();

        // change the rendered content (different bin_path) so an update is needed
        let new_ctx = RenderCtx {
            bin_name: "wsmr".into(),
            bin_path: "/usr/local/bin/wsmr".into(),
            waitpid_bin: "waitpid".into(),
        };
        let plan = plan_generate(td.path(), &new_ctx, &dropin_input(), true).unwrap();
        assert!(plan.conflicts.is_empty());
        assert!(plan.writes.iter().any(|w| w.relname == unit.name));
    }

    #[test]
    fn tampered_manifest_entry_is_a_conflict() {
        let td = TempDir::new();
        std::fs::write(
            td.path().join(templates::GRAPH[0].name),
            "not what the manifest says we wrote\n",
        )
        .unwrap();
        let mut manifest = Manifest::default();
        // manifest claims ownership, but the fingerprint won't match the disk content
        manifest.record(
            templates::GRAPH[0].name,
            "totally different original content\n",
        );
        manifest.save(td.path()).unwrap();

        let plan = plan_generate(td.path(), &ctx(), &dropin_input(), true).unwrap();
        assert!(
            plan.conflicts
                .iter()
                .any(|c| c.relname == templates::GRAPH[0].name)
        );
    }

    #[test]
    fn identical_foreign_content_is_left_alone_and_unclaimed() {
        let td = TempDir::new();
        let content = templates::render(templates::GRAPH[0].body, &ctx());
        // simulate a file another tool wrote with byte-identical content, no manifest at all
        std::fs::write(td.path().join(templates::GRAPH[0].name), &content).unwrap();

        let plan = plan_generate(td.path(), &ctx(), &dropin_input(), true).unwrap();
        assert!(plan.conflicts.is_empty());
        assert!(
            !plan
                .writes
                .iter()
                .any(|w| w.relname == templates::GRAPH[0].name)
        );
    }

    #[test]
    fn foreign_file_at_a_managed_dropin_path_is_not_a_conflict_on_removal_plan() {
        let td = TempDir::new();
        let relname = "wayland-wm@sway.service.d/50_custom.conf";
        std::fs::create_dir_all(td.path().join("wayland-wm@sway.service.d")).unwrap();
        std::fs::write(td.path().join(relname), "not ours\n").unwrap();

        // minimal input needs no drop-in -> wsmr would want to "clean up" this
        // path, but it doesn't own it, so it must be left untouched and not
        // reported as blocking.
        let minimal = DropinInput {
            id: "sway".into(),
            id_unit_string: "sway".into(),
            bin_path: "/usr/bin/wsmr".into(),
            bin_name: "sway".into(),
            desktop_names: vec!["sway".into()],
            cmdline: vec!["sway".into()],
            ..Default::default()
        };
        let plan = plan_generate(td.path(), &ctx(), &minimal, true).unwrap();
        assert!(plan.conflicts.is_empty());
        assert!(!plan.removes.iter().any(|r| r.relname == relname));
        assert_eq!(
            std::fs::read_to_string(td.path().join(relname)).unwrap(),
            "not ours\n"
        );
    }

    #[test]
    fn plan_remove_all_never_touches_disk_and_ignores_graph_units() {
        let td = TempDir::new();
        let mut manifest = Manifest::default();
        manifest.record(templates::GRAPH[0].name, "x");
        manifest.record("wayland-wm@sway.service.d/50_custom.conf", "y");
        manifest.save(td.path()).unwrap();
        std::fs::write(td.path().join(templates::GRAPH[0].name), "x").unwrap();
        std::fs::create_dir_all(td.path().join("wayland-wm@sway.service.d")).unwrap();
        std::fs::write(
            td.path().join("wayland-wm@sway.service.d/50_custom.conf"),
            "y",
        )
        .unwrap();

        let before = std::fs::read_to_string(td.path().join(templates::GRAPH[0].name)).unwrap();
        let plan = plan_remove_all(td.path(), None).unwrap();
        assert_eq!(
            std::fs::read_to_string(td.path().join(templates::GRAPH[0].name)).unwrap(),
            before
        );
        assert!(
            !plan
                .removes
                .iter()
                .any(|r| r.relname == templates::GRAPH[0].name)
        );
        assert!(
            plan.removes
                .iter()
                .any(|r| r.relname == "wayland-wm@sway.service.d/50_custom.conf")
        );
    }

    #[test]
    fn plan_remove_all_skips_drifted_entries() {
        let td = TempDir::new();
        let relname = "wayland-wm@sway.service.d/50_custom.conf";
        let mut manifest = Manifest::default();
        manifest.record(relname, "original\n");
        manifest.save(td.path()).unwrap();
        std::fs::create_dir_all(td.path().join("wayland-wm@sway.service.d")).unwrap();
        std::fs::write(td.path().join(relname), "edited by someone else\n").unwrap();

        let plan = plan_remove_all(td.path(), None).unwrap();
        assert!(plan.removes.is_empty());
        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(plan.skipped[0].relname, relname);
    }
}
