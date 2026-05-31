//! `check may-start`: startup precondition checks. Ports `check_may_start`
//! (`main.py:4535`). Returns a verdict of error/visible/silent messages; the
//! caller decides exit status. See analysis §1.

use crate::error::{Error, Result};
use crate::session::stop;
use crate::sysd::dbus::{SessionBus, SystemBus};
use crate::util;
use std::time::Duration;

/// Inputs to [`check_may_start`].
pub struct CheckOpts {
    /// Skip the "is a login shell" check.
    pub no_login: bool,
    /// Allowed VTs (default `[1]`); `[0]` disables all VT checks.
    pub vtnr: Vec<u32>,
    /// Allow a remote session.
    pub allow_remote: bool,
    /// Wait this long for the system `graphical.target` (0 = skip).
    pub gst_seconds: u64,
    /// Report all checks, not just the first failure.
    pub verbose: bool,
}

/// Outcome: hard errors, visible dealbreakers, and silent dealbreakers.
#[derive(Default)]
pub struct Verdict {
    /// Errors encountered while checking.
    pub errors: Vec<String>,
    /// Reasons shown by default.
    pub visible: Vec<String>,
    /// Reasons shown only when verbose.
    pub silent: Vec<String>,
}

impl Verdict {
    /// Whether the compositor may start (no errors or dealbreakers).
    pub fn may_start(&self) -> bool {
        self.errors.is_empty() && self.visible.is_empty() && self.silent.is_empty()
    }
}

/// Run the precondition checks.
pub fn check_may_start(opts: &CheckOpts) -> Verdict {
    let mut v = Verdict::default();

    // SAFETY: geteuid is always safe.
    if unsafe { libc::geteuid() } == 0 {
        v.silent.push("Running as root".into());
        return v;
    }

    if std::env::var("DBUS_SESSION_BUS_ADDRESS")
        .map(|s| s.is_empty())
        .unwrap_or(true)
    {
        v.silent
            .push("DBUS_SESSION_BUS_ADDRESS is not available".into());
        return v;
    }

    if !opts.no_login {
        match parent_cmdline() {
            Ok(cmd) if !cmd.starts_with('-') => {
                v.silent.push("Not in login shell".into());
                if !opts.verbose {
                    return v;
                }
            }
            Ok(_) => {}
            Err(e) => {
                v.errors
                    .push(format!("Could not determine parent process command: {e}"));
                return v;
            }
        }
    }

    match SessionBus::connect().and_then(|b| stop::is_active(&b)) {
        Ok(true) => {
            v.visible
                .push("A compositor and/or graphical-session* targets are already active".into());
            if !opts.verbose {
                return v;
            }
        }
        Ok(false) => {}
        Err(e) => {
            v.errors
                .push(format!("Could not check for active compositor: {e}"));
            return v;
        }
    }

    let vt_checks = opts.vtnr != [0];

    if vt_checks {
        match util::read_fg_vt() {
            Ok(fgvt) if !opts.vtnr.contains(&fgvt) => {
                v.silent.push(format!(
                    "Foreground VT ({fgvt}) is not among allowed VTs ({})",
                    join_vts(&opts.vtnr)
                ));
                if !opts.verbose {
                    return v;
                }
            }
            Ok(_) => {}
            Err(_) => {
                v.silent.push("Could not determine foreground VT".into());
                if !opts.verbose {
                    return v;
                }
            }
        }
    }

    let session_id = std::env::var("XDG_SESSION_ID").unwrap_or_default();
    let need_system = vt_checks || !opts.allow_remote || opts.gst_seconds > 0;
    let sysbus = if need_system {
        match SystemBus::connect() {
            Ok(b) => Some(b),
            Err(e) => {
                v.errors
                    .push(format!("Could not connect to the system bus: {e}"));
                return v;
            }
        }
    } else {
        None
    };

    if vt_checks {
        if session_id.is_empty() {
            v.silent.push("XDG_SESSION_ID is not available".into());
            if !opts.verbose {
                return v;
            }
        } else if let Some(b) = &sysbus {
            match b.session_vtnr(&session_id) {
                Ok(0) => {
                    v.silent
                        .push(format!("Session {session_id} is not associated with a VT"));
                    if !opts.verbose {
                        return v;
                    }
                }
                Ok(vt) if !opts.vtnr.contains(&vt) => {
                    v.silent.push(format!(
                        "Session VT ({vt}) is not among allowed VTs ({})",
                        join_vts(&opts.vtnr)
                    ));
                    if !opts.verbose {
                        return v;
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    v.errors.push(format!("Could not get session VTNr: {e}"));
                    return v;
                }
            }
        }
    }

    if !opts.allow_remote {
        if session_id.is_empty() {
            v.silent.push("XDG_SESSION_ID is not available".into());
            if !opts.verbose {
                return v;
            }
        } else if let Some(b) = &sysbus {
            match b.session_remote(&session_id) {
                Ok(true) => {
                    v.silent.push(format!("Session {session_id} is not local"));
                    if !opts.verbose {
                        return v;
                    }
                }
                Ok(false) => {}
                Err(e) => {
                    v.errors
                        .push(format!("Could not get session Remote attr: {e}"));
                    return v;
                }
            }
        }
    }

    if opts.gst_seconds > 0
        && let Some(b) = &sysbus
    {
        match b.wait_for_unit(
            "graphical.target",
            &["active", "activating"],
            Duration::from_secs(opts.gst_seconds),
        ) {
            Ok(true) => {}
            Ok(false) => {
                v.silent
                    .push("System has not reached graphical.target".into());
                if !opts.verbose {
                    return v;
                }
            }
            Err(e) => {
                v.errors.push(format!(
                    "Could not check if graphical.target is reached: {e}"
                ));
                return v;
            }
        }
    }

    v
}

fn parent_cmdline() -> Result<String> {
    // SAFETY: getppid is always safe.
    let ppid = unsafe { libc::getppid() };
    let path = format!("/proc/{ppid}/cmdline");
    std::fs::read_to_string(&path).map_err(|e| Error::io(path, e))
}

fn join_vts(vts: &[u32]) -> String {
    vts.iter().map(u32::to_string).collect::<Vec<_>>().join("|")
}
