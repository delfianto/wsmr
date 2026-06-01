//! `check may-start`: startup precondition checks. Ports `check_may_start`
//! (`main.py:4535`). Returns a verdict of error/visible/silent messages; the
//! caller decides exit status. See analysis §1.
//!
//! The side-effecting probes (uid, env, `/proc`, the session/system buses, the
//! foreground VT) are abstracted behind [`Probes`] so the branching logic is
//! unit-testable without a live systemd/D-Bus. [`check_may_start`] wires in the
//! real [`SysProbes`]; tests inject a fake.

use crate::error::{Error, Result};
use crate::session::stop;
use crate::sysd::dbus::{SessionBus, SystemBus};
use crate::util;
use std::cell::RefCell;
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

/// Side-effecting probes used by [`check_may_start_with`]. Abstracted so the
/// decision logic can be exercised with injected values in tests.
pub trait Probes {
    /// Whether the effective UID is root.
    fn is_root(&self) -> bool;
    /// Whether `DBUS_SESSION_BUS_ADDRESS` is set and non-empty.
    fn dbus_session_present(&self) -> bool;
    /// The parent process command line (`/proc/<ppid>/cmdline`, NUL-trimmed).
    fn parent_cmdline(&self) -> Result<String>;
    /// Whether a compositor / `graphical-session*` target is already active.
    fn compositor_active(&self) -> Result<bool>;
    /// The foreground VT number.
    fn foreground_vt(&self) -> Result<u32>;
    /// `$XDG_SESSION_ID` (empty if unset).
    fn session_id(&self) -> String;
    /// Establish the system-bus connection (surfaces a connect error early).
    fn connect_system(&self) -> Result<()>;
    /// The logind session's `VTNr` (0 = not associated with a VT).
    fn session_vtnr(&self, session_id: &str) -> Result<u32>;
    /// Whether the logind session is `Remote`.
    fn session_remote(&self, session_id: &str) -> Result<bool>;
    /// Whether the system `graphical.target` is reached within `secs`.
    fn graphical_target_reached(&self, secs: u64) -> Result<bool>;
}

/// Run the precondition checks (wires in the real [`SysProbes`]).
pub fn check_may_start(opts: &CheckOpts) -> Verdict {
    check_may_start_with(opts, &SysProbes::new())
}

/// Run the precondition checks against an arbitrary [`Probes`] implementation.
pub fn check_may_start_with(opts: &CheckOpts, p: &impl Probes) -> Verdict {
    let mut v = Verdict::default();

    if p.is_root() {
        v.silent.push("Running as root".into());
        return v;
    }

    if !p.dbus_session_present() {
        v.silent
            .push("DBUS_SESSION_BUS_ADDRESS is not available".into());
        return v;
    }

    if !opts.no_login {
        match p.parent_cmdline() {
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

    match p.compositor_active() {
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
        match p.foreground_vt() {
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

    let session_id = p.session_id();
    let need_system = vt_checks || !opts.allow_remote || opts.gst_seconds > 0;
    if need_system && let Err(e) = p.connect_system() {
        v.errors
            .push(format!("Could not connect to the system bus: {e}"));
        return v;
    }

    if vt_checks {
        if session_id.is_empty() {
            v.silent.push("XDG_SESSION_ID is not available".into());
            if !opts.verbose {
                return v;
            }
        } else {
            match p.session_vtnr(&session_id) {
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
        } else {
            match p.session_remote(&session_id) {
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

    if opts.gst_seconds > 0 {
        match p.graphical_target_reached(opts.gst_seconds) {
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

/// The real probe implementation: actual uid/env/`/proc`/D-Bus calls.
pub struct SysProbes {
    sysbus: RefCell<Option<SystemBus>>,
}

impl SysProbes {
    /// Construct a probe set with no system-bus connection yet.
    pub fn new() -> SysProbes {
        SysProbes {
            sysbus: RefCell::new(None),
        }
    }
}

impl Default for SysProbes {
    fn default() -> Self {
        Self::new()
    }
}

impl Probes for SysProbes {
    fn is_root(&self) -> bool {
        // SAFETY: geteuid is always safe.
        let uid = unsafe { libc::geteuid() };
        uid == 0
    }

    fn dbus_session_present(&self) -> bool {
        std::env::var("DBUS_SESSION_BUS_ADDRESS")
            .map(|s| !s.is_empty())
            .unwrap_or(false)
    }

    fn parent_cmdline(&self) -> Result<String> {
        // SAFETY: getppid is always safe.
        let ppid = unsafe { libc::getppid() };
        let path = format!("/proc/{ppid}/cmdline");
        std::fs::read_to_string(&path).map_err(|e| Error::io(path, e))
    }

    fn compositor_active(&self) -> Result<bool> {
        let bus = SessionBus::connect()?;
        stop::is_active(&bus)
    }

    fn foreground_vt(&self) -> Result<u32> {
        util::read_fg_vt()
    }

    fn session_id(&self) -> String {
        std::env::var("XDG_SESSION_ID").unwrap_or_default()
    }

    fn connect_system(&self) -> Result<()> {
        let bus = SystemBus::connect()?;
        *self.sysbus.borrow_mut() = Some(bus);
        Ok(())
    }

    fn session_vtnr(&self, session_id: &str) -> Result<u32> {
        self.sysbus
            .borrow()
            .as_ref()
            .expect("connect_system before session_vtnr")
            .session_vtnr(session_id)
    }

    fn session_remote(&self, session_id: &str) -> Result<bool> {
        self.sysbus
            .borrow()
            .as_ref()
            .expect("connect_system before session_remote")
            .session_remote(session_id)
    }

    fn graphical_target_reached(&self, secs: u64) -> Result<bool> {
        self.sysbus
            .borrow()
            .as_ref()
            .expect("connect_system before graphical_target_reached")
            .wait_for_unit(
                "graphical.target",
                &["active", "activating"],
                Duration::from_secs(secs),
            )
    }
}

fn join_vts(vts: &[u32]) -> String {
    vts.iter().map(u32::to_string).collect::<Vec<_>>().join("|")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// A fully-scriptable [`Probes`] for exercising the decision logic.
    struct Fake {
        root: bool,
        dbus: bool,
        parent: Result<String>,
        active: Result<bool>,
        fg_vt: Result<u32>,
        session_id: String,
        connect: Result<()>,
        vtnr: Result<u32>,
        remote: Result<bool>,
        gst: Result<bool>,
        connected: Cell<bool>,
    }

    impl Default for Fake {
        fn default() -> Self {
            Fake {
                root: false,
                dbus: true,
                parent: Ok("-bash".into()), // a login shell by default
                active: Ok(false),
                fg_vt: Ok(1),
                session_id: "1".into(),
                connect: Ok(()),
                vtnr: Ok(1),
                remote: Ok(false),
                gst: Ok(true),
                connected: Cell::new(false),
            }
        }
    }

    // Result is not Clone; hand out fresh copies for the trait's by-value returns.
    fn dup<T: Clone>(r: &Result<T>) -> Result<T> {
        match r {
            Ok(v) => Ok(v.clone()),
            Err(_) => Err(Error::Resolve("probe error".into())),
        }
    }

    impl Probes for Fake {
        fn is_root(&self) -> bool {
            self.root
        }
        fn dbus_session_present(&self) -> bool {
            self.dbus
        }
        fn parent_cmdline(&self) -> Result<String> {
            dup(&self.parent)
        }
        fn compositor_active(&self) -> Result<bool> {
            dup(&self.active)
        }
        fn foreground_vt(&self) -> Result<u32> {
            dup(&self.fg_vt)
        }
        fn session_id(&self) -> String {
            self.session_id.clone()
        }
        fn connect_system(&self) -> Result<()> {
            self.connected.set(true);
            dup(&self.connect)
        }
        fn session_vtnr(&self, _: &str) -> Result<u32> {
            assert!(self.connected.get(), "connect_system must precede vtnr");
            dup(&self.vtnr)
        }
        fn session_remote(&self, _: &str) -> Result<bool> {
            dup(&self.remote)
        }
        fn graphical_target_reached(&self, _: u64) -> Result<bool> {
            dup(&self.gst)
        }
    }

    fn opts() -> CheckOpts {
        CheckOpts {
            no_login: false,
            vtnr: vec![1],
            allow_remote: false,
            gst_seconds: 0,
            verbose: false,
        }
    }

    #[test]
    fn all_clear_may_start() {
        let v = check_may_start_with(&opts(), &Fake::default());
        assert!(
            v.may_start(),
            "errors={:?} visible={:?} silent={:?}",
            v.errors,
            v.visible,
            v.silent
        );
    }

    #[test]
    fn root_is_silent_dealbreaker() {
        let f = Fake {
            root: true,
            ..Default::default()
        };
        let v = check_may_start_with(&opts(), &f);
        assert!(!v.may_start());
        assert_eq!(v.silent, ["Running as root"]);
    }

    #[test]
    fn missing_dbus_is_dealbreaker() {
        let f = Fake {
            dbus: false,
            ..Default::default()
        };
        assert!(!check_may_start_with(&opts(), &f).may_start());
    }

    #[test]
    fn not_login_shell_unless_no_login() {
        let f = Fake {
            parent: Ok("bash".into()), // no leading '-'
            ..Default::default()
        };
        assert!(!check_may_start_with(&opts(), &f).may_start());
        // --no-login skips the check
        let mut o = opts();
        o.no_login = true;
        assert!(
            check_may_start_with(
                &o,
                &Fake {
                    parent: Ok("bash".into()),
                    ..Default::default()
                }
            )
            .may_start()
        );
    }

    #[test]
    fn parent_cmdline_error_is_hard_error() {
        let f = Fake {
            parent: Err(Error::Resolve("x".into())),
            ..Default::default()
        };
        let v = check_may_start_with(&opts(), &f);
        assert_eq!(v.errors.len(), 1);
    }

    #[test]
    fn active_compositor_is_visible_dealbreaker() {
        let f = Fake {
            active: Ok(true),
            ..Default::default()
        };
        let v = check_may_start_with(&opts(), &f);
        assert_eq!(v.visible.len(), 1);
    }

    #[test]
    fn active_check_error() {
        let f = Fake {
            active: Err(Error::Resolve("x".into())),
            ..Default::default()
        };
        assert_eq!(check_may_start_with(&opts(), &f).errors.len(), 1);
    }

    #[test]
    fn foreground_vt_not_allowed() {
        let f = Fake {
            fg_vt: Ok(3), // allowed is [1]
            ..Default::default()
        };
        assert!(!check_may_start_with(&opts(), &f).may_start());
    }

    #[test]
    fn foreground_vt_unknown_is_silent() {
        let f = Fake {
            fg_vt: Err(Error::Resolve("no vt".into())),
            ..Default::default()
        };
        let v = check_may_start_with(&opts(), &f);
        assert!(v.silent.iter().any(|s| s.contains("foreground VT")));
    }

    #[test]
    fn vt_checks_disabled_with_zero() {
        // vtnr=[0] disables VT + skips session-VTNr; allow_remote skips remote.
        let o = CheckOpts {
            vtnr: vec![0],
            allow_remote: true,
            ..opts()
        };
        let f = Fake {
            fg_vt: Err(Error::Resolve("x".into())), // would fail, but skipped
            ..Default::default()
        };
        assert!(check_may_start_with(&o, &f).may_start());
        assert!(!f.connected.get(), "no system bus needed");
    }

    #[test]
    fn system_bus_connect_error() {
        let f = Fake {
            connect: Err(Error::Resolve("nope".into())),
            ..Default::default()
        };
        assert_eq!(check_may_start_with(&opts(), &f).errors.len(), 1);
    }

    #[test]
    fn session_not_on_vt_and_vtnr_mismatch_and_error() {
        // VTNr 0 → not associated
        let f = Fake {
            vtnr: Ok(0),
            ..Default::default()
        };
        assert!(!check_may_start_with(&opts(), &f).may_start());
        // VTNr not in allowed
        let f = Fake {
            vtnr: Ok(5),
            ..Default::default()
        };
        assert!(!check_may_start_with(&opts(), &f).may_start());
        // VTNr probe error
        let f = Fake {
            vtnr: Err(Error::Resolve("x".into())),
            ..Default::default()
        };
        assert_eq!(check_may_start_with(&opts(), &f).errors.len(), 1);
    }

    #[test]
    fn missing_session_id_under_vt_checks() {
        let f = Fake {
            session_id: String::new(),
            ..Default::default()
        };
        assert!(!check_may_start_with(&opts(), &f).may_start());
    }

    #[test]
    fn remote_session_rejected_and_error() {
        let f = Fake {
            remote: Ok(true),
            ..Default::default()
        };
        assert!(!check_may_start_with(&opts(), &f).may_start());
        let f = Fake {
            remote: Err(Error::Resolve("x".into())),
            ..Default::default()
        };
        assert_eq!(check_may_start_with(&opts(), &f).errors.len(), 1);
    }

    #[test]
    fn graphical_target_checks() {
        let mut o = opts();
        o.gst_seconds = 1;
        // reached → fine
        assert!(check_may_start_with(&o, &Fake::default()).may_start());
        // not reached → silent dealbreaker
        let f = Fake {
            gst: Ok(false),
            ..Default::default()
        };
        assert!(!check_may_start_with(&o, &f).may_start());
        // probe error → hard error
        let f = Fake {
            gst: Err(Error::Resolve("x".into())),
            ..Default::default()
        };
        assert_eq!(check_may_start_with(&o, &f).errors.len(), 1);
    }

    #[test]
    fn verbose_collects_all_failures() {
        // many failing checks at once; verbose keeps going and records each
        let o = CheckOpts {
            verbose: true,
            ..opts()
        };
        let f = Fake {
            parent: Ok("bash".into()), // not login shell
            fg_vt: Ok(9),              // wrong VT
            vtnr: Ok(0),               // not on a VT
            remote: Ok(true),          // remote
            ..Default::default()
        };
        let v = check_may_start_with(&o, &f);
        assert!(v.silent.len() >= 4, "got {:?}", v.silent);
    }
}
