//! Command-line surface (clap derive), mirroring uwsm's argparse tree minus the
//! compositor `select` subcommand (out of scope — SDDM handles selection).
//! See `docs/uwsm-core-analysis.md` §1.

use clap::{Args, Parser, Subcommand, ValueEnum};

/// Top-level CLI.
#[derive(Parser, Debug)]
#[command(
    name = "wsmr",
    version,
    about = "Wayland Session Manager in Rust (a uwsm port)"
)]
pub struct Cli {
    /// Subcommand to run.
    #[command(subcommand)]
    pub command: Command,
}

/// Top-level subcommands.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Start a Wayland compositor session.
    Start(StartArgs),
    /// Stop the running compositor session.
    Stop(StopArgs),
    /// Export variables and signal readiness (run by the compositor).
    Finalize(FinalizeArgs),
    /// Launch an application as a scope/service unit.
    App(AppArgs),
    /// Session state checks.
    Check(CheckArgs),
    /// Internal helpers (invoked by the systemd user manager).
    Aux(AuxArgs),
}

/// Where unit files are written.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rung {
    /// `$XDG_RUNTIME_DIR/systemd/user`.
    Runtime,
    /// `$XDG_CONFIG_HOME/systemd/user`.
    Home,
}

/// Type of unit launched apps run as.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppUnitType {
    /// Run in the caller's lifecycle as a `.scope`.
    Scope,
    /// Run as a managed `.service`.
    Service,
}

/// What to silence on launched apps.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Silence {
    /// Silence stdout.
    Out,
    /// Silence stderr.
    Err,
    /// Silence both.
    Both,
}

/// `start` arguments.
#[derive(Args, Debug)]
pub struct StartArgs {
    /// Only (re)generate unit files, then exit.
    #[arg(short = 'o', long = "only-generate")]
    pub only_generate: bool,
    /// Dry run: show what would happen without doing it.
    #[arg(short = 'n', long = "dry-run")]
    pub dry_run: bool,
    /// Compositor display name (metadata).
    #[arg(short = 'N', long = "name")]
    pub wm_name: Option<String>,
    /// Compositor description (metadata).
    #[arg(short = 'C', long = "comment")]
    pub wm_comment: Option<String>,
    /// Colon-separated desktop names (sets XDG_CURRENT_DESKTOP).
    #[arg(short = 'D', long = "desktop-names")]
    pub desktop_names: Option<String>,
    /// Treat `-D` names as exclusive (don't merge from environment).
    #[arg(short = 'e', long = "exclusive")]
    pub desktop_names_exclusive: bool,
    /// Hardcode the resolved compositor path into the unit.
    #[arg(short = 'a', long = "hardcode")]
    pub hardcode: bool,
    /// Where to write generated unit files.
    #[arg(short = 'U', long = "unit-rung", default_value = "runtime")]
    pub unit_rung: Rung,
    /// Disable tweak drop-ins.
    #[arg(long = "no-tweaks")]
    pub no_tweaks: bool,
    /// Compositor command (or wayland-sessions entry) and its arguments.
    #[arg(
        required = true,
        trailing_var_arg = true,
        num_args = 1..,
        value_name = "COMPOSITOR [ARGS...]"
    )]
    pub wm_cmdline: Vec<String>,
}

/// `stop` arguments.
#[derive(Args, Debug)]
pub struct StopArgs {
    /// Dry run.
    #[arg(short = 'n', long = "dry-run")]
    pub dry_run: bool,
    /// Remove generated unit files after stopping (optionally a comma list of marks).
    #[arg(short = 'r', long = "remove", num_args = 0..=1, default_missing_value = "")]
    pub remove: Option<String>,
    /// Which rung to remove units from.
    #[arg(short = 'U', long = "unit-rung", default_value = "runtime")]
    pub unit_rung: Rung,
}

/// `finalize` arguments.
#[derive(Args, Debug)]
pub struct FinalizeArgs {
    /// Additional variable names to export.
    #[arg(value_name = "VAR")]
    pub env_names: Vec<String>,
}

/// `app` arguments.
#[derive(Args, Debug)]
pub struct AppArgs {
    /// Target slice: `a` (app), `b` (background), `s` (session) or `custom.slice`.
    #[arg(
        short = 's',
        long = "slice",
        default_value = "a",
        value_name = "{a,b,s,custom.slice}"
    )]
    pub slice_name: String,
    /// Unit type (overridable by `$UWSM_APP_UNIT_TYPE`).
    #[arg(short = 't', long = "type", default_value = "scope")]
    pub app_unit_type: AppUnitType,
    /// Launch in a terminal.
    #[arg(short = 'T', long = "terminal")]
    pub terminal: bool,
    /// Application name (unit name fragment).
    #[arg(short = 'a', long = "app-name")]
    pub app_name: Option<String>,
    /// Explicit unit name.
    #[arg(short = 'u', long = "unit-name")]
    pub unit_name: Option<String>,
    /// Unit description.
    #[arg(short = 'd', long = "description")]
    pub unit_description: Option<String>,
    /// Extra `KEY=VALUE` systemd unit properties (repeatable).
    #[arg(long = "unit-property", value_name = "KEY=VALUE")]
    pub unit_properties: Vec<String>,
    /// Silence app output (`--silent` alone means both).
    #[arg(long = "silent", num_args = 0..=1, require_equals = true, default_missing_value = "both")]
    pub silent: Option<Silence>,
    /// Command (or desktop entry `id[:action]`) and its arguments.
    #[arg(trailing_var_arg = true, num_args = 0.., value_name = "CMD [ARGS...]")]
    pub cmdline: Vec<String>,
}

/// `check` arguments.
#[derive(Args, Debug)]
pub struct CheckArgs {
    /// Which check to run.
    #[command(subcommand)]
    pub what: CheckCmd,
}

/// `check` subcommands.
#[derive(Subcommand, Debug)]
pub enum CheckCmd {
    /// Whether a compositor / graphical session is active.
    IsActive(IsActiveArgs),
    /// Whether a compositor may be started in this context.
    MayStart(MayStartArgs),
}

/// `check is-active` arguments.
#[derive(Args, Debug)]
pub struct IsActiveArgs {
    /// Verbose output.
    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,
    /// Optional specific compositor id to check.
    #[arg(value_name = "WM")]
    pub wm: Option<String>,
}

/// `check may-start` arguments.
#[derive(Args, Debug)]
pub struct MayStartArgs {
    /// Verbose output.
    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,
    /// Suppress non-fatal output.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,
}

/// `aux` arguments.
#[derive(Args, Debug)]
pub struct AuxArgs {
    /// Which internal action to run.
    #[command(subcommand)]
    pub action: AuxAction,
}

/// `aux` internal actions (only run by the systemd user manager).
#[derive(Subcommand, Debug)]
pub enum AuxAction {
    /// Prepare the activation environment (env-preloader service).
    PrepareEnv(AuxIdArgs),
    /// Clean up the activation environment (ExecStopPost).
    CleanupEnv,
    /// Exec the compositor with the autoready watcher.
    Exec(AuxIdArgs),
    /// Wait for a PID to exit.
    Waitpid(WaitpidArgs),
    /// Wait for variables to appear in the activation environment.
    Waitenv(WaitenvArgs),
    /// Fast application argument generator daemon.
    AppDaemon,
}

/// Arguments for `aux prepare-env` / `aux exec`.
#[derive(Args, Debug)]
pub struct AuxIdArgs {
    /// Colon-separated desktop names (used by prepare-env).
    #[arg(short = 'D', long = "desktop-names")]
    pub desktop_names: Option<String>,
    /// Treat `-D` names as exclusive.
    #[arg(short = 'e', long = "exclusive")]
    pub desktop_names_exclusive: bool,
    /// Compositor display name.
    #[arg(short = 'N', long = "name")]
    pub wm_name: Option<String>,
    /// Compositor description.
    #[arg(short = 'C', long = "comment")]
    pub wm_comment: Option<String>,
    /// Compositor id.
    #[arg(value_name = "WM_ID")]
    pub wm_id: String,
    /// Optional raw command line.
    #[arg(trailing_var_arg = true, num_args = 0..)]
    pub wm_cmdline: Vec<String>,
}

/// Arguments for `aux waitpid`.
#[derive(Args, Debug)]
pub struct WaitpidArgs {
    /// PID to wait on.
    #[arg(value_name = "PID")]
    pub pid: i32,
}

/// Arguments for `aux waitenv`.
#[derive(Args, Debug)]
pub struct WaitenvArgs {
    /// Variable names to wait for (in addition to WAYLAND_DISPLAY).
    #[arg(value_name = "VAR")]
    pub env_names: Vec<String>,
}
