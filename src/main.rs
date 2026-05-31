//! wsmr binary entrypoint: parse the CLI and dispatch.
//!
//! Implemented this pass: `start --only-generate` (drives unit generation) and
//! compositor resolution for the bare-executable case. The remaining
//! orchestration (full start/stop/app/aux/finalize/check) is stubbed with
//! [`wsmr::error::Error::NotImplemented`] and lands in later milestones.

use anyhow::Result;
use clap::Parser;
use wsmr::cli::{
    AppArgs, AuxAction, AuxArgs, CheckArgs, CheckCmd, Cli, Command, FinalizeArgs, Rung as CliRung,
    StartArgs, StopArgs,
};
use wsmr::comp::{CompGlobals, ResolveInput};
use wsmr::error::{Error, Result as WResult};
use wsmr::units::generate::{self, Rung};
use wsmr::units::templates::{DropinInput, RenderCtx};

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
    let input = ResolveInput {
        wm_cmdline: args.wm_cmdline.clone(),
        desktop_names: split_colon(args.desktop_names.as_deref().unwrap_or_default()),
        desktop_names_exclusive: args.desktop_names_exclusive,
        name: args.wm_name.clone(),
        description: args.wm_comment.clone(),
        xdg_current_desktop: split_colon(&std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default()),
    };
    let cg = CompGlobals::resolve(&input)?;

    println!("Resolved compositor:");
    println!("  id            : {}", cg.id);
    println!("  unit id       : {}", cg.id_unit_string);
    println!("  bin_id        : {}", cg.bin_id);
    println!("  desktop names : {}", cg.desktop_names.join(":"));
    println!("  command       : {}", cg.cmdline.join(" "));

    if args.only_generate {
        let rung = match args.unit_rung {
            CliRung::Runtime => Rung::Runtime,
            CliRung::Home => Rung::Home,
        };
        let dir = generate::rung_dir(rung)?;
        let ctx = render_ctx()?;
        let dropins = DropinInput {
            id: cg.id.clone(),
            id_unit_string: cg.id_unit_string.clone(),
            bin_path: ctx.bin_path.clone(),
            bin_name: cg.bin_name.clone(),
            name: cg.name.clone(),
            description: cg.description.clone(),
            desktop_names: cg.desktop_names.clone(),
            cli_desktop_names: split_colon(args.desktop_names.as_deref().unwrap_or_default()),
            cli_desktop_names_exclusive: args.desktop_names_exclusive,
            cmdline: cg.cmdline.clone(),
            cli_args: cg.cmdline.iter().skip(1).cloned().collect(),
        };
        let out = generate::generate(&dir, &ctx, &dropins)?;
        println!("\nGenerated units in {}", dir.display());
        if out.changed {
            for w in &out.written {
                println!("  + {w}");
            }
            for r in &out.removed {
                println!("  - {r}");
            }
        } else {
            println!("  (unchanged)");
        }
        return Ok(());
    }

    Err(Error::todo("M3", "session start"))
}

fn stop(_args: StopArgs) -> WResult<()> {
    Err(Error::todo("M4", "session stop"))
}

fn finalize(_args: FinalizeArgs) -> WResult<()> {
    Err(Error::todo("M6", "finalize"))
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
    let what = match args.action {
        AuxAction::PrepareEnv(_) => "aux prepare-env",
        AuxAction::CleanupEnv => "aux cleanup-env",
        AuxAction::Exec(_) => "aux exec",
        AuxAction::Waitpid(_) => "aux waitpid",
        AuxAction::Waitenv(_) => "aux waitenv",
        AuxAction::AppDaemon => "aux app-daemon",
    };
    Err(Error::todo("M3", what))
}

fn split_colon(s: &str) -> Vec<String> {
    if s.is_empty() {
        Vec::new()
    } else {
        s.split(':').map(str::to_string).collect()
    }
}

fn render_ctx() -> WResult<RenderCtx> {
    let exe = std::env::current_exe().map_err(|e| Error::io("current_exe", e))?;
    Ok(RenderCtx {
        bin_name: "wsmr".into(),
        bin_path: exe.to_string_lossy().into_owned(),
        waitpid_bin: "waitpid".into(),
    })
}
