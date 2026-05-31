//! `stop` and `check is-active`. Ports `stop_wm` (`main.py:4391`) and
//! `is_active` (`main.py:1189`). See `REFERENCE.md` §8.3.

use crate::error::Result;
use crate::sysd::dbus::SessionBus;
use crate::units::generate::{self, Rung};

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
pub fn run_stop(opts: &StopOpts) -> Result<()> {
    let bus = SessionBus::connect()?;
    if !stop_wm(&bus, opts.dry_run)? {
        println!("Compositor is not running.");
    }

    if opts.remove.is_some() {
        let dir = generate::rung_dir(opts.rung)?;
        let outcome = generate::remove_all(&dir)?;
        for r in &outcome.removed {
            println!("  - {r}");
        }
        if outcome.changed && !opts.dry_run {
            bus.reload()?;
        }
    }
    Ok(())
}
