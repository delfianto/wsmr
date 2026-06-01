//! `finalize`: export variables to the activation environments and signal
//! readiness. Ports `finalize` (`main.py:2424`). See `REFERENCE.md` §4.2.

use crate::env::files;
use crate::error::{Error, Result};
use crate::filter;
use crate::session::runtime_path;
use crate::sysd::dbus::SessionBus;
use std::collections::BTreeMap;
use std::os::unix::process::CommandExt;
use std::process::Command;

/// Export `WAYLAND_DISPLAY`, `DISPLAY` and any `extra_vars` (read from the
/// current environment), record them for cleanup, then `exec systemd-notify` to
/// declare readiness. Returns only on failure (success path replaces the process).
pub fn finalize(extra_vars: &[String]) -> Result<()> {
    if std::env::var("WAYLAND_DISPLAY")
        .map(|v| v.is_empty())
        .unwrap_or(true)
    {
        return Err(Error::Resolve(
            "WAYLAND_DISPLAY is not set — are we being run by a Wayland compositor?".into(),
        ));
    }

    let bus = SessionBus::connect()?;

    let mut names: Vec<String> = vec!["WAYLAND_DISPLAY".into(), "DISPLAY".into()];
    names.extend(extra_vars.iter().cloned());
    let mut export: BTreeMap<String, String> = BTreeMap::new();
    for name in names {
        if filter::keep_name(&name)
            && let Ok(val) = std::env::var(&name)
        {
            export.insert(name, val);
        }
    }

    files::append_cleanup(&runtime_path("env_cleanup.list")?, export.keys().cloned())?;
    bus.set_systemd_vars(&export)?;

    // If the compositor unit is still activating, declare ready; otherwise just
    // narrow notification access (if it's still wide open).
    let activating = bus.list_units_by_patterns(&["activating"], &["wayland-wm@*.service"])?;
    if !activating.is_empty() {
        return Err(exec_systemd_notify(&["READY=1", "NOTIFYACCESS=exec"]));
    }
    if let Some(unit) = bus.active_wm_unit()?
        && bus.service_notify_access(&unit)? == "all"
    {
        return Err(exec_systemd_notify(&["NOTIFYACCESS=exec"]));
    }
    Ok(())
}

/// `exec systemd-notify <args>`. Only returns (an error) if exec fails.
fn exec_systemd_notify(args: &[&str]) -> Error {
    let mut cmd = Command::new("systemd-notify");
    cmd.args(args);
    crate::coverage::flush_before_exec();
    let err = cmd.exec();
    Error::io("systemd-notify", err)
}
