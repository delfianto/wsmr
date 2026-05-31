//! wsmr binary entrypoint: parse the CLI and dispatch.
//!
//! All subcommands are wired to real logic (`session::*`, `app::*`). Linux-only
//! runtime paths are verified via the Podman integration harness. Deliberately
//! deferred bits (e.g. desktop-entry *compositor* resolution) return
//! `Error::NotImplemented`.

use anyhow::Result;
use clap::Parser;
use wsmr::cli::{
    AppArgs, AuxAction, AuxArgs, AuxIdArgs, CheckArgs, CheckCmd, Cli, Command, FinalizeArgs,
    Rung as CliRung, StartArgs, StopArgs,
};
use wsmr::comp::{CompGlobals, ResolveInput};
use wsmr::error::{Error, Result as WResult};
use wsmr::session::{self, start::StartOpts};
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
    let comp = CompGlobals::resolve(&ResolveInput {
        wm_cmdline: args.wm_cmdline.clone(),
        desktop_names: split_colon(args.desktop_names.as_deref().unwrap_or_default()),
        desktop_names_exclusive: args.desktop_names_exclusive,
        name: args.wm_name.clone(),
        description: args.wm_comment.clone(),
        xdg_current_desktop: split_colon(&std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default()),
    })?;
    let opts = StartOpts {
        only_generate: args.only_generate,
        dry_run: args.dry_run,
        rung: rung(args.unit_rung),
        gst_timeout: None, // TODO: wire --gst-* flags
        bin_path: current_exe()?,
    };
    session::start::run(&comp, &opts)
}

fn stop(args: StopArgs) -> WResult<()> {
    session::stop::run_stop(&session::stop::StopOpts {
        dry_run: args.dry_run,
        remove: args.remove,
        rung: rung(args.unit_rung),
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
            let active = session::stop::is_active(&SessionBus::connect()?)?;
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

fn split_colon(s: &str) -> Vec<String> {
    if s.is_empty() {
        Vec::new()
    } else {
        s.split(':').map(str::to_string).collect()
    }
}

fn current_exe() -> WResult<String> {
    Ok(std::env::current_exe()
        .map_err(|e| Error::io("current_exe", e))?
        .to_string_lossy()
        .into_owned())
}
