//! `aux exec`: spawn the readiness watcher as an independent child, then exec
//! the compositor in the unit's cgroup. Ports the `aux exec` handler
//! (`main.py:5066`). See `REFERENCE.md` §4.1/§5.
//!
//! uwsm forks + double-forks the watcher. We **spawn** it instead: zbus's
//! async-io reactor thread does not survive `fork()`, so a forked watcher's
//! D-Bus connection is dead and never sends `READY=1` (found via the Tier-B
//! integration test). A freshly spawned `wsmr aux readiness <id>` process gets a
//! clean address space + its own reactor, and — being started before the
//! compositor exec — lives in the same service cgroup, so its `systemd-notify`
//! is accepted under `NotifyAccess=all`.

use crate::comp::CompGlobals;
use crate::env::files;
use crate::error::{Error, Result};
use crate::session::{runtime_path, wait};
use crate::sysd::dbus::SessionBus;
use std::collections::BTreeMap;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::time::Duration;

/// Spawn the readiness watcher, then exec the compositor (replacing this
/// process).
pub fn aux_exec(comp: &CompGlobals) -> Result<()> {
    let self_exe = std::env::current_exe().map_err(|e| Error::io("current_exe", e))?;
    // independent child in our cgroup; inherits $NOTIFY_SOCKET for systemd-notify
    Command::new(&self_exe)
        .args(["aux", "readiness", &comp.id])
        .spawn()
        .map_err(|e| Error::io("spawn readiness watcher", e))?;
    exec_compositor(&comp.cmdline)
}

/// The readiness watcher (`aux readiness <id>`): snapshot the activation env,
/// wait for the expected vars, sync the delta, then `exec systemd-notify` to
/// declare readiness. Returns only on error.
pub fn readiness_watch(comp: &CompGlobals) -> Result<()> {
    let bus = SessionBus::connect()?;
    let env_pre = bus.systemd_vars()?;
    let unit = format!("wayland-wm@{}.service", comp.id_unit_string);

    let timeout = bus
        .service_timeout_start_usec(&unit)
        .ok()
        .map(Duration::from_micros)
        .unwrap_or_else(wait::wait_timeout);

    let mut vars = vec!["WAYLAND_DISPLAY".to_string()];
    vars.extend(
        std::env::var("UWSM_WAIT_VARNAMES")
            .unwrap_or_default()
            .split_whitespace()
            .map(String::from),
    );
    wait::waitenv(&bus, &vars, timeout)?;
    std::thread::sleep(wait::settle_time());

    // sync env delta to the activation environment + cleanup list
    let env_post = bus.systemd_vars()?;
    let mut delta: BTreeMap<String, String> = BTreeMap::new();
    for (k, v) in &env_post {
        if env_pre.get(k) != Some(v) {
            delta.insert(k.clone(), v.clone());
        }
    }
    if !delta.is_empty() {
        bus.set_systemd_vars(&delta)?;
        files::append_cleanup(&runtime_path("env_cleanup.list")?, delta.keys().cloned())?;
    }

    // declare readiness if still activating; else narrow access if wide open
    let activating = bus.list_units_by_patterns(&["activating"], &["wayland-wm@*.service"])?;
    if !activating.is_empty() {
        return Err(exec_systemd_notify(&["READY=1", "NOTIFYACCESS=exec"]));
    }
    if bus.active_wm_unit()?.is_some()
        && bus
            .service_notify_access(&unit)
            .map(|s| s == "all")
            .unwrap_or(false)
    {
        return Err(exec_systemd_notify(&["NOTIFYACCESS=exec"]));
    }
    Ok(())
}

fn exec_compositor(cmdline: &[String]) -> Result<()> {
    let prog = cmdline
        .first()
        .ok_or_else(|| Error::InvalidArg("empty compositor command line".into()))?;
    let mut cmd = Command::new(prog);
    cmd.args(&cmdline[1..]);
    crate::coverage::flush_before_exec();
    let err = cmd.exec();
    Err(Error::io(prog.clone(), err))
}

fn exec_systemd_notify(args: &[&str]) -> Error {
    let mut cmd = Command::new("systemd-notify");
    cmd.args(args);
    crate::coverage::flush_before_exec();
    let err = cmd.exec();
    Error::io("systemd-notify", err)
}
