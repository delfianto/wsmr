//! `stop` and `check is-active`. Ports `stop_wm` (`main.py:4391`) and
//! `is_active` (`main.py:1189`). See `REFERENCE.md` §8.3.

use crate::error::Result;
use crate::sysd::dbus::SessionBus;
use crate::units::generate::{self, Rung};
use crate::units::plan::{RemovalPlan, plan_remove_all};
use std::path::Path;
use std::time::Duration;

/// How long `stop_wm` waits for the compositor's stop job to clear before
/// giving up. Generously above `wayland-wm@.service`'s own
/// `TimeoutStopSec=10`, since the wait covers the whole cascading session
/// teardown, not just that one unit.
const STOP_JOB_TIMEOUT: Duration = Duration::from_secs(20);

/// The upstream "generic" active-check unit set: any of these being
/// active/activating means a session is up or coming up. Ports
/// `check_units_generic` from `is_active` (`main.py:1207`) — narrower sets
/// (just `wayland-wm@*.service`, or a single escaped instance) miss the
/// window where a session is mid-startup in `*-pre@.target`.
const GENERIC_ACTIVE_PATTERNS: &[&str] = &[
    "graphical-session-pre.target",
    "wayland-session-pre@*.target",
    "graphical-session.target",
    "wayland-session@*.target",
    "wayland-wm@*.service",
];

/// Whether a compositor or graphical session is active/activating.
pub fn is_active(bus: &SessionBus) -> Result<bool> {
    is_active_for(bus, None)
}

/// Whether the named compositor (or, with `wm_id: None`, any
/// compositor/graphical-session unit) is active/activating. Ports `is_active`
/// (`main.py:1189`) including its `check_wm_id` selector — used by both the
/// double-start refusal (`None`) and `check is-active <WM>` (`Some`).
pub fn is_active_for(bus: &SessionBus, wm_id: Option<&str>) -> Result<bool> {
    match wm_id {
        None => Ok(!bus
            .list_units_by_patterns(&["active", "activating"], GENERIC_ACTIVE_PATTERNS)?
            .is_empty()),
        Some(id) => {
            let unit = compositor_unit_name(id);
            Ok(!bus
                .list_units_by_patterns(&["active", "activating"], &[unit.as_str()])?
                .is_empty())
        }
    }
}

/// The escaped `wayland-wm@<id>.service` unit name for compositor `id`.
/// Ports the `check_wm_id` branch of `is_active` (`main.py:1218`).
pub fn compositor_unit_name(id: &str) -> String {
    format!(
        "wayland-wm@{}.service",
        crate::units::escape::simple_systemd_escape(id, false)
    )
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
    bus.wait_for_job(&job, &unit.name, STOP_JOB_TIMEOUT)?;
    Ok(true)
}

/// Options for [`run_stop`].
pub struct StopOpts {
    /// Dry run.
    pub dry_run: bool,
    /// `-r`: remove generated units after stopping. `Some("")` (bare `-r`)
    /// removes everything wsmr owns; `Some("id,tweaks")` removes only the
    /// listed marks (see [`parse_marks`]); `None` means don't remove at all.
    pub remove: Option<String>,
    /// Rung to remove units from.
    pub rung: Rung,
}

/// Parse `-r`'s raw value into a mark filter for [`plan_remove_all`]: an
/// empty string means "no filter" (`None`, remove everything removable);
/// otherwise a comma-separated list of marks (a compositor id, or
/// `"tweaks"`), matching upstream's `-r` value shape (`main.py:1933`).
fn parse_marks(raw: &str) -> Option<Vec<String>> {
    let marks: Vec<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    (!marks.is_empty()).then_some(marks)
}

/// Run the `stop` command.
///
/// `--dry-run --remove` is strictly read-only: the removal plan is computed
/// and reported without deleting anything or reloading the manager.
pub fn run_stop(opts: &StopOpts) -> Result<()> {
    let bus = SessionBus::connect()?;
    if !stop_wm(&bus, opts.dry_run)? {
        println!("Compositor is not running.");
    }

    if let Some(raw_marks) = &opts.remove {
        let dir = generate::rung_dir(opts.rung)?;
        let marks = parse_marks(raw_marks);
        let plan = plan_remove_all(&dir, marks.as_deref())?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compositor_unit_name_escapes_and_wraps() {
        assert_eq!(compositor_unit_name("sway"), "wayland-wm@sway.service");
        // matches simple_systemd_escape(id, start=False): a leading dot is
        // NOT escaped here (start=false), unlike a unit-instance start.
        assert_eq!(
            compositor_unit_name("my comp"),
            "wayland-wm@my\\x20comp.service"
        );
        assert_eq!(compositor_unit_name("a/b"), "wayland-wm@a-b.service");
    }

    #[test]
    fn parse_marks_empty_means_no_filter() {
        assert_eq!(parse_marks(""), None);
        assert_eq!(parse_marks("  , ,"), None);
    }

    #[test]
    fn parse_marks_splits_and_trims() {
        assert_eq!(
            parse_marks("sway, tweaks ,,hyprland"),
            Some(vec![
                "sway".to_string(),
                "tweaks".to_string(),
                "hyprland".to_string()
            ])
        );
    }
}
