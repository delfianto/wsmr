//! `stop` and `check is-active`. Ports `stop_wm` (`main.py:4391`) and
//! `is_active` (`main.py:1189`). See `REFERENCE.md` §8.3.

use crate::error::Result;
use crate::sysd::dbus::SessionBus;
use crate::units::generate::{self, Rung};
use crate::units::plan::{RemovalPlan, plan_remove_all};
use std::path::Path;

/// Whether a compositor or graphical session is active/activating.
pub fn is_active(bus: &SessionBus) -> Result<bool> {
    if !bus
        .list_units_by_patterns(&["active", "activating"], &["wayland-wm@*.service"])?
        .is_empty()
    {
        return Ok(true);
    }
    Ok(!bus
        .list_units_by_patterns(&["active", "activating"], &["graphical-session.target"])?
        .is_empty())
}

/// Stop the running compositor (which cascades the whole session teardown).
/// Returns true if a compositor was found and a stop job issued.
pub fn stop_wm(bus: &SessionBus, dry_run: bool) -> Result<bool> {
    let units = bus.list_units_by_patterns(&["active", "activating"], &["wayland-wm@*.service"])?;
    let Some(unit) = units.into_iter().next() else {
        return Ok(false);
    };
    if dry_run {
        println!("Would stop {}.", unit.name);
        return Ok(true);
    }
    let job = bus.stop_unit(&unit.name, "fail")?;
    bus.wait_for_job(&job)?;
    Ok(true)
}

/// Options for [`run_stop`].
pub struct StopOpts {
    /// Dry run.
    pub dry_run: bool,
    /// `-r`: remove generated units after stopping (value is reserved for a
    /// future mark filter; presence means "remove").
    pub remove: Option<String>,
    /// Rung to remove units from.
    pub rung: Rung,
}

/// Run the `stop` command.
///
/// `--dry-run --remove` is strictly read-only (P0-02): the removal plan is
/// computed and reported without deleting anything or reloading the manager.
pub fn run_stop(opts: &StopOpts) -> Result<()> {
    let bus = SessionBus::connect()?;
    if !stop_wm(&bus, opts.dry_run)? {
        println!("Compositor is not running.");
    }

    if opts.remove.is_some() {
        let dir = generate::rung_dir(opts.rung)?;
        let plan = plan_remove_all(&dir)?;

        if opts.dry_run {
            report_removal_plan(&dir, &plan);
            return Ok(());
        }

        for skipped in &plan.skipped {
            eprintln!(
                "warning: leaving {} untouched \u{2014} {}",
                skipped.relname, skipped.reason
            );
        }
        let outcome = generate::apply_removal(&dir, plan)?;
        for r in &outcome.removed {
            println!("  - {r}");
        }
        if outcome.changed {
            bus.reload()?;
        }
    }
    Ok(())
}

fn report_removal_plan(dir: &Path, plan: &RemovalPlan) {
    println!("Dry run: removal in {}", dir.display());
    if plan.removes.is_empty() && plan.skipped.is_empty() {
        println!("  (nothing to remove)");
        return;
    }
    for r in &plan.removes {
        println!("  - {}", r.relname);
    }
    for s in &plan.skipped {
        println!("  ! {} (left in place \u{2014} {})", s.relname, s.reason);
    }
    if !plan.removes.is_empty() {
        println!("  (would reload the systemd user manager)");
    }
}
