//! `start`: generate units, refuse double-start, bind to our PID, snapshot the
//! login environment, and become the session anchor by exec-ing the shell
//! signal handler on the session envelope target. Ports the `start` dispatch
//! (`main.py:4719`) + exec chain (`:4894`). See `REFERENCE.md` §3.1/§9.
//!
//! **Linux-runtime; unverified until the integration phase.**

use crate::comp::CompGlobals;
use crate::env::files;
use crate::error::{Error, Result};
use crate::session::{helpers, runtime_path};
use crate::sysd::dbus::{SessionBus, SystemBus};
use crate::units::generate::{self, GenOutcome, Rung};
use crate::units::plan::{GenerationPlan, plan_generate};
use crate::units::templates::{DropinInput, RenderCtx};
use crate::varnames;
use std::collections::BTreeMap;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// System `graphical.target` gate, driven by `start -g`/`-G`. Ports the
/// mutually exclusive `gst_warn_seconds`/`gst_abort_seconds` handling
/// (`main.py:1890`/`:4709`) — `-G` (abort) takes precedence over `-g` (warn)
/// when both would apply; a negative value disables its own gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GstGate {
    /// Don't check at all.
    Disabled,
    /// Wait up to this long; on timeout, warn and continue anyway.
    Warn(Duration),
    /// Wait up to this long; on timeout, refuse to start.
    Abort(Duration),
}

/// Flags controlling `start`.
pub struct StartOpts {
    /// Only (re)generate units, then exit.
    pub only_generate: bool,
    /// Dry run.
    pub dry_run: bool,
    /// Where to write unit files.
    pub rung: Rung,
    /// System `graphical.target` gate (skipped entirely for `only_generate`
    /// or `dry_run`, matching upstream).
    pub gst_gate: GstGate,
    /// Generate the fixed tweak drop-ins (`start -t`/`-T`).
    pub tweaks: bool,
    /// Absolute path to the wsmr executable (for generated `ExecStart=`).
    pub bin_path: String,
}

/// Run the start flow for `comp`.
///
/// Ordering is safety-critical: every read-only eligibility check — the
/// system-target gate, the double-start refusal, and computing the
/// generation plan — runs to completion *before* anything on disk or in the
/// systemd user manager is touched. A refusal (already active, or a plan
/// conflict) or `--dry-run` therefore never generates, writes, or reloads.
pub fn run(comp: &CompGlobals, opts: &StartOpts) -> Result<()> {
    // (1) optional system graphical.target gate (read-only). Skipped for
    // only-generate/dry-run, matching upstream (`main.py:4710-4713`) — those
    // modes don't actually start anything, so gating them on system state
    // that has no bearing on what they do would just be noise.
    if !opts.only_generate && !opts.dry_run {
        gst_gate(opts.gst_gate)?;
    }

    // (2) refuse double start (read-only) before generating or reloading
    // anything.
    let bus = SessionBus::connect()?;
    refuse_if_active(crate::session::stop::is_active(&bus)?)?;

    // (3) compute the generation plan (read-only: stats/reads existing files
    // and the ownership manifest, never writes).
    let dir = generate::rung_dir(opts.rung)?;
    let ctx = RenderCtx {
        bin_name: "wsmr".into(),
        bin_path: opts.bin_path.clone(),
        waitpid_bin: "waitpid".into(),
    };
    let plan = plan_generate(
        &dir,
        &ctx,
        &build_dropins(comp, &opts.bin_path),
        opts.tweaks,
    )?;

    // Dry-run always reports the full plan — including any conflicts — before
    // any error is raised, so `--dry-run` is a strict superset of what a real
    // run would tell you, never less informative because it would have failed.
    if opts.dry_run {
        report_plan(&dir, &plan);
    }
    if !plan.conflicts.is_empty() {
        return Err(generate::conflict_error(&dir, &plan.conflicts));
    }
    if opts.dry_run {
        println!("Dry run: would start {}.", comp.id);
        return Ok(());
    }

    // (4) apply the plan for real, then reload only if it actually changed
    // something.
    let outcome = generate::apply_generate(&dir, plan)?;
    if outcome.changed {
        bus.reload()?;
    }

    if opts.only_generate {
        report(&dir, &outcome);
        return Ok(());
    }

    // (5) bind the graphical session to our PID
    let pid = std::process::id();
    let status = Command::new("systemctl")
        .args([
            "--user",
            "start",
            &format!("wayland-session-bindpid@{pid}.service"),
        ])
        .status()
        .map_err(|e| Error::io("systemctl", e))?;
    if !status.success() {
        return Err(Error::Resolve("failed to start the bindpid unit".into()));
    }

    // (6) snapshot login environment for the preloader + units
    save_login_envs()?;

    // (7) become the session anchor: preserve real stdout/stderr on fd 3/4, then
    // replace ourselves with systemd-cat -> sh signal-handler.sh <envelope>
    let script = helpers::extract("signal-handler.sh")?;
    // SAFETY: duplicating our own already-open stdout/stderr fds to 3/4 (both
    // valid targets — plain integers, not file handles, so nothing else can
    // invalidate them between the two calls) so the shell handler can
    // message past systemd-cat (which captures fd 1/2 into the journal). A
    // failure means the *messaging* path breaks, not the session itself, but
    // it must still be reported rather than silently exec-ing into a signal
    // handler that can't talk to the user.
    unsafe {
        if libc::dup2(1, 3) < 0 {
            return Err(Error::io("dup2(1, 3)", std::io::Error::last_os_error()));
        }
        if libc::dup2(2, 4) < 0 {
            return Err(Error::io("dup2(2, 4)", std::io::Error::last_os_error()));
        }
    }
    let envelope = format!("wayland-session-envelope@{}.target", comp.id_unit_string);
    let mut cmd = Command::new("systemd-cat");
    cmd.args([
        "--identifier=wsmr",
        "--stderr-priority=err",
        "--",
        "/bin/sh",
    ])
    .arg(&script)
    .arg(&envelope);
    crate::coverage::flush_before_exec();
    let err = cmd.exec();
    Err(Error::io("systemd-cat", err))
}

/// Wait on the system `graphical.target` per `gate`, warning-and-continuing
/// or aborting on timeout as configured. A no-op for [`GstGate::Disabled`].
fn gst_gate(gate: GstGate) -> Result<()> {
    let (timeout, abort) = match gate {
        GstGate::Disabled => return Ok(()),
        GstGate::Warn(t) => (t, false),
        GstGate::Abort(t) => (t, true),
    };
    let sysbus = SystemBus::connect()?;
    if sysbus.wait_for_unit("graphical.target", &["active", "activating"], timeout)? {
        return Ok(());
    }
    if abort {
        return Err(Error::Resolve(
            "system has not reached graphical.target; aborting".into(),
        ));
    }
    eprintln!(
        "wsmr: system has not reached graphical.target. It might be a good idea to check the \
         default system target, or screen for this with \"wsmr check may-start\". Continuing in \
         5 seconds..."
    );
    std::thread::sleep(Duration::from_secs(5));
    Ok(())
}

/// Pure refusal check, split out from [`run`] so it's unit-testable without a
/// live session bus: `already_active` is whatever the caller determined by
/// querying systemd (see [`crate::session::stop::is_active`]).
fn refuse_if_active(already_active: bool) -> Result<()> {
    if already_active {
        return Err(Error::Resolve(
            "a compositor or graphical session is already active".into(),
        ));
    }
    Ok(())
}

fn report_plan(dir: &Path, plan: &GenerationPlan) {
    println!("Dry run: units in {}", dir.display());
    if plan.is_empty() && plan.conflicts.is_empty() {
        println!("  (unchanged)");
        return;
    }
    for w in &plan.writes {
        println!("  + {}", w.relname);
    }
    for r in &plan.removes {
        println!("  - {}", r.relname);
    }
    for c in &plan.conflicts {
        println!("  ! {} (blocked \u{2014} {})", c.relname, c.reason);
    }
    if !plan.conflicts.is_empty() {
        println!("  would refuse: paths above are not verifiably owned by wsmr");
    } else if !plan.is_empty() {
        println!("  (would reload the systemd user manager)");
    }
}

fn build_dropins(comp: &CompGlobals, bin_path: &str) -> DropinInput {
    DropinInput {
        id: comp.id.clone(),
        id_unit_string: comp.id_unit_string.clone(),
        bin_path: bin_path.to_string(),
        bin_name: comp.bin_name.clone(),
        name: comp.name.clone(),
        description: comp.description.clone(),
        desktop_names: comp.desktop_names.clone(),
        cli_desktop_names: comp.cli_desktop_names.clone(),
        cli_desktop_names_exclusive: comp.cli_desktop_names_exclusive,
        cmdline: comp.cmdline.clone(),
        cli_args: comp.cmdline.iter().skip(1).cloned().collect(),
    }
}

fn save_login_envs() -> Result<()> {
    let environ: BTreeMap<String, String> = std::env::vars().collect();
    files::save_env(&runtime_path("env_login")?, &environ, files::Sep::Nul)?;
    let sess: BTreeMap<String, String> = varnames::SESSION_SPECIFIC
        .iter()
        .filter_map(|k| {
            std::env::var(*k)
                .ok()
                .filter(|v| !v.is_empty())
                .map(|v| ((*k).to_string(), v))
        })
        .collect();
    files::save_env(
        &runtime_path("env_session.conf")?,
        &sess,
        files::Sep::Newline,
    )
}

fn report(dir: &Path, outcome: &GenOutcome) {
    println!("Generated units in {}", dir.display());
    if outcome.changed {
        for w in &outcome.written {
            println!("  + {w}");
        }
        for r in &outcome.removed {
            println!("  - {r}");
        }
    } else {
        println!("  (unchanged)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuse_if_active_blocks_only_when_active() {
        assert!(refuse_if_active(false).is_ok());
        let err = refuse_if_active(true).unwrap_err();
        assert!(err.to_string().contains("already active"));
    }

    #[test]
    fn build_dropins_maps_comp_fields() {
        let comp = CompGlobals {
            cmdline: vec!["/usr/bin/sway".into(), "--unsupported-gpu".into()],
            id: "sway".into(),
            id_unit_string: "sway".into(),
            bin_name: "sway".into(),
            bin_id: "sway".into(),
            desktop_names: vec!["sway".into()],
            name: Some("Sway".into()),
            description: None,
            cli_desktop_names: vec!["sway".into()],
            cli_desktop_names_exclusive: true,
        };
        let d = build_dropins(&comp, "/usr/bin/wsmr");
        assert_eq!(d.id, "sway");
        assert_eq!(d.bin_path, "/usr/bin/wsmr");
        assert_eq!(d.cmdline, vec!["/usr/bin/sway", "--unsupported-gpu"]);
        assert_eq!(d.cli_args, vec!["--unsupported-gpu"]);
        assert_eq!(d.desktop_names, vec!["sway"]);
        // CLI -D/-e are threaded through verbatim (not approximated)
        assert_eq!(d.cli_desktop_names, vec!["sway"]);
        assert!(d.cli_desktop_names_exclusive);
    }
}
