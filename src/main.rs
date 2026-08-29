//! wsmr binary entrypoint: parse the CLI and dispatch.
//!
//! All subcommands are wired to real logic (`session::*`, `app::*`). Linux-only
//! runtime paths are verified via the Podman integration harness. Deliberately
//! deferred bits (e.g. desktop-entry *compositor* resolution) return
//! `Error::NotImplemented`.

use anyhow::Result;
use clap::Parser;
use std::time::Duration;
use wsmr::cli::{
    AppArgs, AuxAction, AuxArgs, AuxIdArgs, CheckArgs, CheckCmd, Cli, Command, FinalizeArgs,
    Rung as CliRung, StartArgs, StopArgs,
};
use wsmr::comp::{CompGlobals, ResolveInput};
use wsmr::error::{Error, Result as WResult};
use wsmr::session::{
    self,
    start::{GstGate, StartOpts},
};
use wsmr::sysd::dbus::SessionBus;
use wsmr::units::generate::Rung;

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Start(args) => start(args)?,
        Command::Stop(args) => stop(args)?,
        Command::Finalize(args) => finalize(args)?,
        Command::App(args) => app(args)?,
        Command::Check(args) => check(args)?,
        Command::Aux(args) => aux(args)?,
    }
    Ok(())
}

fn start(args: StartArgs) -> WResult<()> {
    let mut comp = CompGlobals::resolve(&ResolveInput {
        wm_cmdline: args.wm_cmdline.clone(),
        desktop_names: split_colon(args.desktop_names.as_deref().unwrap_or_default()),
        desktop_names_exclusive: args.desktop_names_exclusive,
        name: args.wm_name.clone(),
        description: args.wm_comment.clone(),
        xdg_current_desktop: split_colon(&std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default()),
    })?;
    apply_hardcode(&mut comp, args.hardcode)?;
    let opts = StartOpts {
        only_generate: args.only_generate,
        dry_run: args.dry_run,
        rung: resolve_rung(args.unit_rung),
        gst_gate: resolve_gst_gate(args.gst_warn_seconds, args.gst_abort_seconds),
        tweaks: resolve_tweaks(args.no_tweaks, args.tweaks),
        bin_path: current_exe()?,
    };
    session::start::run(&comp, &opts)
        .inspect_err(|e| session::log_error_to_journal(&format!("wsmr: start failed: {e}")))
}

fn stop(args: StopArgs) -> WResult<()> {
    session::stop::run_stop(&session::stop::StopOpts {
        dry_run: args.dry_run,
        remove: args.remove,
        rung: resolve_rung(args.unit_rung),
    })
}

fn finalize(args: FinalizeArgs) -> WResult<()> {
    let mut vars = args.env_names;
    vars.extend(
        std::env::var("UWSM_FINALIZE_VARNAMES")
            .unwrap_or_default()
            .split_whitespace()
            .map(String::from),
    );
    session::finalize::finalize(&vars)
        .inspect_err(|e| session::notify_error("wsmr: finalize failed", &e.to_string()))
}

fn app(args: AppArgs) -> WResult<()> {
    wsmr::app::launch::run(args.into())
        .inspect_err(|e| session::notify_error("wsmr: app launch failed", &e.to_string()))
}

fn check(args: CheckArgs) -> WResult<()> {
    match args.what {
        CheckCmd::IsActive(a) => {
            let active = session::stop::is_active_for(&SessionBus::connect()?, a.wm.as_deref())?;
            if a.verbose {
                println!("{}", if active { "active" } else { "inactive" });
            }
            if !active {
                std::process::exit(1);
            }
            Ok(())
        }
        CheckCmd::MayStart(a) => {
            let vtnr = if a.vtnr.is_empty() {
                vec![1]
            } else {
                a.vtnr.clone()
            };
            let verdict = session::check::check_may_start(&session::check::CheckOpts {
                no_login: a.no_login,
                vtnr,
                allow_remote: a.allow_remote,
                gst_seconds: a.gst_seconds,
                verbose: a.verbose,
            });
            if verdict.may_start() {
                if a.verbose {
                    println!("May start compositor.");
                }
                return Ok(());
            }
            if !a.quiet {
                let mut msgs = verdict.errors;
                msgs.extend(verdict.visible);
                if a.verbose {
                    msgs.extend(verdict.silent);
                }
                for m in msgs {
                    eprintln!("{m}");
                }
            }
            std::process::exit(1);
        }
    }
}

fn aux(args: AuxArgs) -> WResult<()> {
    match args.action {
        AuxAction::PrepareEnv(a) => session::prepare::prepare_env(&resolve_aux(&a)?),
        AuxAction::CleanupEnv => session::cleanup::cleanup_env(),
        AuxAction::Exec(a) => session::exec::aux_exec(&resolve_aux(&a)?),
        AuxAction::Readiness(a) => session::exec::readiness_watch(&resolve_aux(&a)?),
        AuxAction::Waitpid(a) => session::wait::waitpid(a.pid),
        AuxAction::Waitenv(a) => {
            let bus = SessionBus::connect()?;
            let mut vars = vec!["WAYLAND_DISPLAY".to_string()];
            vars.extend(a.env_names);
            vars.extend(
                std::env::var("UWSM_WAIT_VARNAMES")
                    .unwrap_or_default()
                    .split_whitespace()
                    .map(String::from),
            );
            session::wait::waitenv(&bus, &vars, session::wait::wait_timeout())
        }
        AuxAction::AppDaemon => wsmr::app::daemon::run(),
    }
}

/// Build a `CompGlobals` for an `aux` action from its id + optional raw cmdline.
fn resolve_aux(args: &AuxIdArgs) -> WResult<CompGlobals> {
    let cmdline = if args.wm_cmdline.is_empty() {
        vec![args.wm_id.clone()]
    } else {
        let mut c = args.wm_cmdline.clone();
        if c[0].is_empty() {
            c[0] = args.wm_id.clone();
        }
        c
    };
    CompGlobals::resolve(&ResolveInput {
        wm_cmdline: cmdline,
        desktop_names: split_colon(args.desktop_names.as_deref().unwrap_or_default()),
        desktop_names_exclusive: args.desktop_names_exclusive,
        name: args.wm_name.clone(),
        description: args.wm_comment.clone(),
        xdg_current_desktop: split_colon(&std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default()),
    })
}

fn rung(r: CliRung) -> Rung {
    match r {
        CliRung::Runtime => Rung::Runtime,
        CliRung::Home => Rung::Home,
    }
}

/// Resolve the unit rung: the CLI flag if given, else `$UWSM_UNIT_RUNG` (only
/// `"run"`/`"home"` are honored; anything else warns and falls back), else
/// `run`. Ports the `unit_rung_default` resolution around `start`'s `-U`
/// (`main.py:1791-1801`).
fn resolve_rung(cli: Option<CliRung>) -> Rung {
    resolve_rung_with(cli, std::env::var("UWSM_UNIT_RUNG").ok())
}

fn resolve_rung_with(cli: Option<CliRung>, env: Option<String>) -> Rung {
    if let Some(r) = cli {
        return rung(r);
    }
    match env.as_deref() {
        Some("run") => Rung::Runtime,
        Some("home") => Rung::Home,
        Some(other) => {
            eprintln!("wsmr: invalid UWSM_UNIT_RUNG value {other:?} ignored, using \"run\".");
            Rung::Runtime
        }
        None => Rung::Runtime,
    }
}

/// Resolve whether tweak drop-ins should be generated: `-t`/`-T` if given,
/// else `$UWSM_TWEAKS` (invalid value warns, falls back to `true`), else the
/// deprecated `$UWSM_NO_TWEAKS` (warns it's deprecated; invalid value warns,
/// falls back to `true`), else `true`. Ports the `tweaks_default` resolution
/// around `start`'s `-t`/`-T` (`main.py:1812-1839`).
fn resolve_tweaks(no_tweaks_flag: bool, tweaks_flag: bool) -> bool {
    resolve_tweaks_with(
        no_tweaks_flag,
        tweaks_flag,
        std::env::var("UWSM_TWEAKS").ok(),
        std::env::var("UWSM_NO_TWEAKS").ok(),
    )
}

fn resolve_tweaks_with(
    no_tweaks_flag: bool,
    tweaks_flag: bool,
    t_env: Option<String>,
    nt_env: Option<String>,
) -> bool {
    if tweaks_flag {
        return true;
    }
    if no_tweaks_flag {
        return false;
    }
    if let Some(t) = t_env {
        return str2bool_plus(&t).unwrap_or_else(|()| {
            eprintln!("wsmr: invalid UWSM_TWEAKS value {t:?} ignored, using true.");
            true
        });
    }
    if let Some(nt) = nt_env {
        eprintln!("wsmr: UWSM_NO_TWEAKS is deprecated and being replaced by UWSM_TWEAKS.");
        return !str2bool_plus(&nt).unwrap_or_else(|()| {
            eprintln!("wsmr: invalid UWSM_NO_TWEAKS value {nt:?} ignored, using true.");
            false
        });
    }
    true
}

/// `str2bool_plus` (non-numeric mode) from `misc.py:13`: numeric strings
/// convert via `> 0`; `""`/`"no"`/`"false"`/`"n"` (case-insensitive) are
/// `false`; `"yes"`/`"true"`/`"y"` are `true`; anything else is rejected.
fn str2bool_plus(s: &str) -> std::result::Result<bool, ()> {
    if let Ok(n) = s.parse::<i64>() {
        return Ok(n > 0);
    }
    match s.to_ascii_lowercase().as_str() {
        "" | "no" | "false" | "n" => Ok(false),
        "yes" | "true" | "y" => Ok(true),
        _ => Err(()),
    }
}

/// Resolve the system-`graphical.target` gate from `-g`/`-G`: `-G` (abort)
/// takes precedence over `-g` (warn) whenever it's non-negative; a negative
/// value disables its own gate. Ports the precedence in `main.py:4715-4720`.
fn resolve_gst_gate(warn_seconds: i64, abort_seconds: i64) -> GstGate {
    if abort_seconds >= 0 {
        GstGate::Abort(Duration::from_secs(abort_seconds as u64))
    } else if warn_seconds >= 0 {
        GstGate::Warn(Duration::from_secs(warn_seconds as u64))
    } else {
        GstGate::Disabled
    }
}

/// `start -F`: canonicalize `comp.cmdline[0]` to an absolute path via `$PATH`
/// lookup, so the generated unit hardcodes the resolved binary rather than a
/// bare name it re-resolves at every launch. A no-op if `comp.cmdline[0]` is
/// already absolute (already-hardcoded implicitly, e.g. the compositor was
/// given as a path). Ports the `Args.parsed.hardcode` branch of
/// `fill_comp_globals` (`main.py:4320-4328`).
fn apply_hardcode(comp: &mut CompGlobals, hardcode: bool) -> WResult<()> {
    if !hardcode {
        return Ok(());
    }
    let Some(first) = comp.cmdline.first() else {
        return Ok(());
    };
    if first.starts_with('/') {
        return Ok(());
    }
    let resolved = wsmr::util::which(first).ok_or_else(|| {
        Error::Resolve(format!(
            "-F/--hardcode was given, but {first:?} was not found on PATH"
        ))
    })?;
    comp.cmdline[0] = path_to_unit_string(&resolved, "the resolved -F/--hardcode executable")?;
    Ok(())
}

fn split_colon(s: &str) -> Vec<String> {
    if s.is_empty() {
        Vec::new()
    } else {
        s.split(':').map(str::to_string).collect()
    }
}

fn current_exe() -> WResult<String> {
    let path = std::env::current_exe().map_err(|e| Error::io("current_exe", e))?;
    path_to_unit_string(&path, "the wsmr executable's own path")
}

/// Convert `path` to a `String` for embedding in a generated unit file
/// (`ExecStart=`, etc.), rejecting it outright rather than silently
/// mangling it if it isn't valid UTF-8. systemd unit files are
/// themselves UTF-8 text, so a lossy conversion here wouldn't just be
/// imprecise — it would write a *different*, likely nonexistent path into
/// the unit, which then silently fails to exec instead of failing here with
/// a clear cause.
fn path_to_unit_string(path: &std::path::Path, what: &str) -> WResult<String> {
    path.to_str().map(str::to_string).ok_or_else(|| {
        Error::InvalidArg(format!(
            "{what} is not valid UTF-8 and cannot be represented in a systemd unit file: {path:?}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_to_unit_string_passes_through_valid_utf8() {
        assert_eq!(
            path_to_unit_string(std::path::Path::new("/usr/bin/sway"), "x").unwrap(),
            "/usr/bin/sway"
        );
    }

    #[test]
    fn path_to_unit_string_rejects_non_utf8() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        // 0xFF is not valid UTF-8 in any position.
        let bytes = [b'/', b'x', b'/', 0xFFu8, b'y'];
        let path = std::path::Path::new(OsStr::from_bytes(&bytes));
        let err = path_to_unit_string(path, "the thing").unwrap_err();
        assert!(err.to_string().contains("the thing"));
        assert!(err.to_string().contains("not valid UTF-8"));
    }

    #[test]
    fn resolve_rung_cli_wins_over_env() {
        assert_eq!(
            resolve_rung_with(Some(CliRung::Home), Some("run".into())),
            Rung::Home
        );
    }

    #[test]
    fn resolve_rung_env_run_and_home() {
        assert_eq!(resolve_rung_with(None, Some("run".into())), Rung::Runtime);
        assert_eq!(resolve_rung_with(None, Some("home".into())), Rung::Home);
    }

    #[test]
    fn resolve_rung_invalid_env_warns_and_falls_back_to_run() {
        // "runtime" is a valid *CLI* alias but not a valid *env var* value
        // upstream would accept — matches main.py's exact env validation.
        assert_eq!(
            resolve_rung_with(None, Some("runtime".into())),
            Rung::Runtime
        );
        assert_eq!(resolve_rung_with(None, Some("bogus".into())), Rung::Runtime);
    }

    #[test]
    fn resolve_rung_no_cli_no_env_defaults_to_run() {
        assert_eq!(resolve_rung_with(None, None), Rung::Runtime);
    }

    #[test]
    fn str2bool_plus_cases() {
        assert_eq!(str2bool_plus("yes"), Ok(true));
        assert_eq!(str2bool_plus("Y"), Ok(true));
        assert_eq!(str2bool_plus("true"), Ok(true));
        assert_eq!(str2bool_plus("no"), Ok(false));
        assert_eq!(str2bool_plus("False"), Ok(false));
        assert_eq!(str2bool_plus(""), Ok(false));
        assert_eq!(str2bool_plus("0"), Ok(false));
        assert_eq!(str2bool_plus("5"), Ok(true));
        assert_eq!(str2bool_plus("-1"), Ok(false));
        assert_eq!(str2bool_plus("banana"), Err(()));
    }

    #[test]
    fn resolve_tweaks_flags_win_over_env() {
        assert!(resolve_tweaks_with(false, true, Some("false".into()), None));
        assert!(!resolve_tweaks_with(true, false, Some("true".into()), None));
    }

    #[test]
    fn resolve_tweaks_reads_uwsm_tweaks() {
        assert!(resolve_tweaks_with(false, false, Some("true".into()), None));
        assert!(!resolve_tweaks_with(
            false,
            false,
            Some("false".into()),
            None
        ));
        // invalid value falls back to true (with a warning, not asserted here)
        assert!(resolve_tweaks_with(
            false,
            false,
            Some("bogus".into()),
            None
        ));
    }

    #[test]
    fn resolve_tweaks_falls_back_to_deprecated_uwsm_no_tweaks() {
        assert!(!resolve_tweaks_with(
            false,
            false,
            None,
            Some("true".into())
        ));
        assert!(resolve_tweaks_with(
            false,
            false,
            None,
            Some("false".into())
        ));
    }

    #[test]
    fn resolve_tweaks_default_is_true() {
        assert!(resolve_tweaks_with(false, false, None, None));
    }

    #[test]
    fn resolve_gst_gate_abort_takes_precedence() {
        assert_eq!(
            resolve_gst_gate(60, 10),
            GstGate::Abort(Duration::from_secs(10))
        );
    }

    #[test]
    fn resolve_gst_gate_warn_when_no_abort() {
        assert_eq!(
            resolve_gst_gate(60, -1),
            GstGate::Warn(Duration::from_secs(60))
        );
    }

    #[test]
    fn resolve_gst_gate_disabled_when_both_negative() {
        assert_eq!(resolve_gst_gate(-1, -1), GstGate::Disabled);
    }

    fn comp(cmdline: &[&str]) -> CompGlobals {
        CompGlobals {
            cmdline: cmdline.iter().map(|s| s.to_string()).collect(),
            id: "x".into(),
            id_unit_string: "x".into(),
            bin_name: "x".into(),
            bin_id: "x".into(),
            desktop_names: vec!["x".into()],
            name: None,
            description: None,
            cli_desktop_names: vec![],
            cli_desktop_names_exclusive: false,
        }
    }

    #[test]
    fn apply_hardcode_noop_when_disabled() {
        let mut c = comp(&["sh"]);
        apply_hardcode(&mut c, false).unwrap();
        assert_eq!(c.cmdline[0], "sh");
    }

    #[test]
    fn apply_hardcode_noop_when_already_absolute() {
        let mut c = comp(&["/bin/sh", "-c", "true"]);
        apply_hardcode(&mut c, true).unwrap();
        assert_eq!(c.cmdline[0], "/bin/sh");
    }

    #[test]
    fn apply_hardcode_resolves_via_path() {
        let mut c = comp(&["sh"]);
        apply_hardcode(&mut c, true).unwrap();
        assert!(c.cmdline[0].starts_with('/'));
        assert!(c.cmdline[0].ends_with("/sh"));
    }

    #[test]
    fn apply_hardcode_errors_when_not_found() {
        let mut c = comp(&["definitely-not-a-real-binary-xyz"]);
        assert!(apply_hardcode(&mut c, true).is_err());
    }
}
