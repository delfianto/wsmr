//! wsmr binary entrypoint: parse the CLI and dispatch to `wsmr::session::*`.
//!
//! M3 wires `start`, `finalize`, and the `aux` actions (`prepare-env`,
//! `cleanup-env`, `exec`, `waitpid`, `waitenv`) to real orchestration. Their
//! Linux-runtime behavior is verified later (integration phase). `app`,
//! `check`, and `aux app-daemon` remain `NotImplemented`.

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

fn stop(_args: StopArgs) -> WResult<()> {
    Err(Error::todo("M4", "session stop"))
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
}

fn app(_args: AppArgs) -> WResult<()> {
    Err(Error::todo("M5", "app launching"))
}

fn check(args: CheckArgs) -> WResult<()> {
    match args.what {
        CheckCmd::IsActive(_) => Err(Error::todo("M4", "check is-active")),
        CheckCmd::MayStart(_) => Err(Error::todo("M6", "check may-start")),
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
        AuxAction::AppDaemon => Err(Error::todo("M7", "app daemon")),
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
