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
use crate::units::templates::{DropinInput, RenderCtx};
use crate::varnames;
use std::collections::BTreeMap;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// Flags controlling `start`.
pub struct StartOpts {
    /// Only (re)generate units, then exit.
    pub only_generate: bool,
    /// Dry run.
    pub dry_run: bool,
    /// Where to write unit files.
    pub rung: Rung,
    /// If set, wait for the system `graphical.target` up to this long first.
    pub gst_timeout: Option<Duration>,
    /// Absolute path to the wsmr executable (for generated `ExecStart=`).
    pub bin_path: String,
}

/// Run the start flow for `comp`.
pub fn run(comp: &CompGlobals, opts: &StartOpts) -> Result<()> {
    // (1) optional system graphical.target gate
    if let Some(timeout) = opts.gst_timeout {
        let sysbus = SystemBus::connect()?;
        if !sysbus.wait_for_unit("graphical.target", &["active", "activating"], timeout)? {
            return Err(Error::Resolve(
                "system has not reached graphical.target".into(),
            ));
        }
    }

    // (2) generate units + per-compositor drop-ins
    let dir = generate::rung_dir(opts.rung)?;
    let ctx = RenderCtx {
        bin_name: "wsmr".into(),
        bin_path: opts.bin_path.clone(),
        waitpid_bin: "waitpid".into(),
    };
    let outcome = generate::generate(&dir, &ctx, &build_dropins(comp, &opts.bin_path))?;

    if opts.only_generate {
        report(&dir, &outcome);
        return Ok(());
    }

    let bus = SessionBus::connect()?;
    if outcome.changed {
        bus.reload()?;
    }

    // (3) refuse double start
    if !bus
        .list_units_by_patterns(&["active", "activating"], &["wayland-wm@*.service"])?
        .is_empty()
    {
        return Err(Error::Resolve(
            "a compositor or graphical session is already active".into(),
        ));
    }

    if opts.dry_run {
        println!("Dry run: would start {}.", comp.id);
        return Ok(());
    }

    // (4) bind the graphical session to our PID
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

    // (5) snapshot login environment for the preloader + units
    save_login_envs()?;

    // (6) become the session anchor: preserve real stdout/stderr on fd 3/4, then
    // replace ourselves with systemd-cat -> sh signal-handler.sh <envelope>
    let script = helpers::extract("signal-handler.sh")?;
    // SAFETY: duplicate std fds to 3/4 so the shell handler can message past
    // systemd-cat (which captures fd 1/2 into the journal).
    unsafe {
        libc::dup2(1, 3);
        libc::dup2(2, 4);
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
