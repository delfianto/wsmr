//! `aux exec`: fork the autoready watcher (double-fork to reparent it away from
//! the compositor), then exec the compositor in the unit's cgroup. Ports the
//! `aux exec` handler (`main.py:5066`). See `REFERENCE.md` §4.1/§5.
//!
//! **Unsafe + Linux-runtime; unverified until the integration phase.**

use crate::comp::CompGlobals;
use crate::env::files;
use crate::error::{Error, Result};
use crate::session::{runtime_path, wait};
use crate::sysd::dbus::SessionBus;
use std::collections::BTreeMap;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::time::Duration;

/// Snapshot the activation env, fork the autoready watcher, then exec the
/// compositor (replacing this process).
pub fn aux_exec(comp: &CompGlobals) -> Result<()> {
    let env_pre = SessionBus::connect()?.systemd_vars()?;

    // SAFETY: fork from the main (single-threaded enough) flow; the child does
    // minimal work then either execs or _exits. The leaf reconnects D-Bus fresh
    // rather than touching parent state.
    match unsafe { nix::unistd::fork() }.map_err(fork_err)? {
        nix::unistd::ForkResult::Parent { child } => {
            // reap the intermediate fork, then fall through to exec the compositor
            let _ = nix::sys::wait::waitpid(child, None);
        }
        nix::unistd::ForkResult::Child => {
            // intermediate: fork again so the leaf is reparented to init
            // SAFETY: as above; every arm diverges (_exit) — never returns.
            match unsafe { nix::unistd::fork() } {
                Ok(nix::unistd::ForkResult::Parent { .. }) => unsafe { libc::_exit(0) },
                Ok(nix::unistd::ForkResult::Child) => {
                    if let Err(e) = watcher(comp, &env_pre) {
                        eprintln!("autoready watcher failed: {e}");
                    }
                    unsafe { libc::_exit(0) }
                }
                Err(_) => unsafe { libc::_exit(1) },
            }
        }
    }

    exec_compositor(&comp.cmdline)
}

/// The autoready watcher: wait for the expected vars, sync the delta, then
/// `exec systemd-notify` to declare readiness. Returns only on error.
fn watcher(comp: &CompGlobals, env_pre: &BTreeMap<String, String>) -> Result<()> {
    let bus = SessionBus::connect()?;
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
    let err = Command::new(prog).args(&cmdline[1..]).exec();
    Err(Error::io(prog.clone(), err))
}

fn exec_systemd_notify(args: &[&str]) -> Error {
    let err = Command::new("systemd-notify").args(args).exec();
    Error::io("systemd-notify", err)
}

fn fork_err(e: nix::errno::Errno) -> Error {
    Error::io("fork", std::io::Error::from_raw_os_error(e as i32))
}
