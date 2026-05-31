//! `wsmr app`: resolve the target (desktop entry or bare command), expand its
//! `Exec`, and launch it as a systemd scope/service in a slice via
//! `systemd-run`. Ports `app()` (`main.py:3335`). Terminal launching (`-T`) and
//! the app-daemon are deferred.

use crate::app::entry::DesktopEntry;
use crate::app::field::{self, GenArgs};
use crate::app::{find, naming};
use crate::comp::MainArg;
use crate::error::{Error, Result};
use crate::util;
use crate::varnames;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

const DESKTOP_ENTRY_VARS: &[&str] = &[
    "DESKTOP_ENTRY_ID",
    "DESKTOP_ENTRY_PATH",
    "DESKTOP_ENTRY_NAME",
    "DESKTOP_ENTRY_NAME_L",
    "DESKTOP_ENTRY_COMMENT",
    "DESKTOP_ENTRY_COMMENT_L",
    "DESKTOP_ENTRY_GENERICNAME",
    "DESKTOP_ENTRY_GENERICNAME_L",
    "DESKTOP_ENTRY_ICON",
    "DESKTOP_ENTRY_ACTION",
    "DESKTOP_ENTRY_ACTION_NAME",
    "DESKTOP_ENTRY_ACTION_NAME_L",
    "DESKTOP_ENTRY_ACTION_ICON",
];

/// Unit type for launched apps.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnitType {
    /// `.scope` in the caller's lifecycle.
    Scope,
    /// managed `.service`.
    Service,
}

impl UnitType {
    fn as_str(self) -> &'static str {
        match self {
            UnitType::Scope => "scope",
            UnitType::Service => "service",
        }
    }
}

/// What to silence on the launched app.
#[derive(Clone, Copy, Debug)]
pub enum Silence {
    /// stdout.
    Out,
    /// stderr.
    Err,
    /// both.
    Both,
}

/// Inputs to [`run`].
pub struct AppOpts {
    /// Command (or entry id/path) + arguments.
    pub cmdline: Vec<String>,
    /// Slice selector: `a`/`b`/`s`/`*.slice`.
    pub slice: String,
    /// Unit type.
    pub unit_type: UnitType,
    /// Launch in a terminal (deferred).
    pub terminal: bool,
    /// Explicit app name.
    pub app_name: Option<String>,
    /// Explicit unit name.
    pub unit_name: Option<String>,
    /// Unit description.
    pub description: Option<String>,
    /// Extra `KEY=VALUE` unit properties.
    pub unit_properties: Vec<String>,
    /// Output to silence.
    pub silent: Option<Silence>,
}

/// Fully-resolved launch parameters fed to [`build_argv`].
pub struct LaunchSpec {
    /// App command line (program at `[0]`).
    pub cmdline: Vec<String>,
    /// Resolved `*.slice`.
    pub slice: String,
    /// Unit type.
    pub unit_type: UnitType,
    /// Unit name.
    pub unit_name: String,
    /// Description.
    pub description: String,
    /// Extra `KEY=VALUE` properties.
    pub properties: Vec<String>,
    /// `--silent` selection (applied via properties for services; via stdio for scopes).
    pub silent: Option<Silence>,
    /// Working directory; `None` → `--same-dir`.
    pub workdir: Option<String>,
    /// `--setenv` session vars (services only).
    pub session_setenv: Vec<(String, String)>,
}

/// Assemble the `systemd-run --user …` argv (pure; unit-tested).
pub fn build_argv(spec: &LaunchSpec) -> Vec<String> {
    let mut a = vec!["systemd-run".to_string(), "--user".to_string()];
    match spec.unit_type {
        UnitType::Scope => a.push("--scope".into()),
        UnitType::Service => {
            a.push("--property=Type=exec".into());
            a.push("--property=ExitType=cgroup".into());
            for (k, v) in &spec.session_setenv {
                a.push(format!("--setenv={k}={v}"));
            }
            match spec.silent {
                Some(Silence::Out) => a.push("--property=StandardOutput=null".into()),
                Some(Silence::Err) => a.push("--property=StandardError=null".into()),
                Some(Silence::Both) => {
                    a.push("--property=StandardOutput=null".into());
                    a.push("--property=StandardError=null".into());
                }
                None => {}
            }
        }
    }
    for p in &spec.properties {
        a.push(format!("--property={p}"));
    }
    a.push(format!("--slice={}", spec.slice));
    a.push(format!("--unit={}", spec.unit_name));
    a.push(format!("--description={}", spec.description));
    a.push("--quiet".into());
    a.push("--collect".into());
    match &spec.workdir {
        Some(d) => a.push(format!("--working-directory={d}")),
        None => a.push("--same-dir".into()),
    }
    a.push("--".into());
    a.extend(spec.cmdline.iter().cloned());
    a
}

/// Launch an application per `opts`.
pub fn run(opts: AppOpts) -> Result<()> {
    for p in &opts.unit_properties {
        if !p.contains('=') {
            return Err(Error::InvalidArg(format!(
                "expected KEY=VALUE unit property, got: {p:?}"
            )));
        }
    }
    if opts.terminal {
        return Err(Error::todo("M5+", "terminal launching"));
    }
    let first = opts
        .cmdline
        .first()
        .ok_or_else(|| Error::InvalidArg("no command given".into()))?;
    let main = MainArg::parse(first)?;

    let mut properties = opts.unit_properties.clone();
    let mut app_name = opts.app_name.clone();
    let mut description = opts.description.clone();
    let mut workdir: Option<String> = None;

    let cmdlines: Vec<Vec<String>> = if let Some(entry_id) = &main.entry_id {
        let entry = load_entry(&main, entry_id)?;
        entry.check_basic(main.entry_action.as_deref())?;
        if entry.terminal() {
            return Err(Error::todo("M5+", "terminal launching"));
        }
        properties.push(format!("SourcePath={}", entry.filename));
        workdir = entry
            .path()
            .map(str::to_string)
            .filter(|d| Path::new(d).is_dir());
        if app_name.is_none() {
            app_name = Some(entry_id.trim_end_matches(".desktop").to_string());
        }
        if description.is_none() {
            description = entry_description(&entry, main.entry_action.as_deref());
        }
        let exec = entry.exec(main.entry_action.as_deref())?;
        let name = entry.get_localized("Name", None).unwrap_or_default();
        let icon = entry.get("Icon", None).unwrap_or("");
        let ctx = field::EntryCtx {
            name: &name,
            icon,
            filename: &entry.filename,
        };
        match field::gen_entry_args(&exec, &opts.cmdline[1..], &ctx)? {
            GenArgs::Single(c) => vec![c],
            GenArgs::Multi(cs) => cs,
        }
    } else {
        // bare executable: adopt DESKTOP_ENTRY_* hints
        if app_name.is_none()
            && let Ok(id) = std::env::var("DESKTOP_ENTRY_ID")
            && !id.is_empty()
        {
            app_name = Some(id.trim_end_matches(".desktop").to_string());
        }
        if let Ok(p) = std::env::var("DESKTOP_ENTRY_PATH")
            && !p.is_empty()
        {
            properties.push(format!("SourcePath={p}"));
        }
        if description.is_none() {
            description = desktop_entry_env_description();
        }
        if util::which(&opts.cmdline[0]).is_none() {
            return Err(Error::Resolve(format!(
                "command not found: {}",
                opts.cmdline[0]
            )));
        }
        vec![opts.cmdline.clone()]
    };

    let slice = resolve_slice(&opts.slice)?;
    let desktop = first_desktop();
    let session_setenv = match opts.unit_type {
        UnitType::Service => session_specific_env(),
        UnitType::Scope => Vec::new(),
    };

    if cmdlines.len() == 1 {
        // single instance → replace this process with systemd-run
        let cmd = cmdlines.into_iter().next().unwrap();
        let unit_name = resolve_unit_name(&opts, &desktop, app_name.as_deref(), &cmd)?;
        let spec = LaunchSpec {
            description: final_description(&description, app_name.as_deref(), &cmd),
            cmdline: cmd,
            slice,
            unit_type: opts.unit_type,
            unit_name,
            properties,
            silent: opts.silent,
            workdir,
            session_setenv,
        };
        let argv = build_argv(&spec);
        let mut command = configure(&argv, opts.unit_type, opts.silent);
        Err(Error::io(argv[0].clone(), command.exec()))
    } else {
        // multi-instance → spawn each, then wait
        let mut children = Vec::new();
        for cmd in cmdlines {
            let unit_name = naming::auto_unit_name(
                opts.unit_type.as_str(),
                &desktop,
                app_name.as_deref().unwrap_or(&basename(&cmd[0])),
                &random_hex8(),
            );
            let spec = LaunchSpec {
                description: final_description(&description, app_name.as_deref(), &cmd),
                cmdline: cmd,
                slice: slice.clone(),
                unit_type: opts.unit_type,
                unit_name,
                properties: properties.clone(),
                silent: opts.silent,
                workdir: workdir.clone(),
                session_setenv: session_setenv.clone(),
            };
            let argv = build_argv(&spec);
            let child = configure(&argv, opts.unit_type, opts.silent)
                .spawn()
                .map_err(|e| Error::io(argv[0].clone(), e))?;
            children.push(child);
        }
        let mut all_ok = true;
        for mut child in children {
            match child.wait() {
                Ok(s) if s.success() => {}
                _ => all_ok = false,
            }
        }
        if all_ok {
            Ok(())
        } else {
            Err(Error::Resolve("one or more app instances failed".into()))
        }
    }
}

fn load_entry(main: &MainArg, entry_id: &str) -> Result<DesktopEntry> {
    match &main.path {
        Some(p) => {
            let content = std::fs::read_to_string(p).map_err(|e| Error::io(p, e))?;
            DesktopEntry::parse(&p.to_string_lossy(), &content)
        }
        None => find::find_entry("applications", entry_id)?
            .ok_or_else(|| Error::Resolve(format!("desktop entry not found: {entry_id}"))),
    }
}

/// Build the `systemd-run` Command: drop `DESKTOP_ENTRY_*` from the child env,
/// and for a silenced scope redirect stdio (services use unit properties).
fn configure(argv: &[String], unit_type: UnitType, silent: Option<Silence>) -> Command {
    let mut c = Command::new(&argv[0]);
    c.args(&argv[1..]);
    for v in DESKTOP_ENTRY_VARS {
        c.env_remove(v);
    }
    if unit_type == UnitType::Scope {
        match silent {
            Some(Silence::Out) => {
                c.stdout(Stdio::null());
            }
            Some(Silence::Err) => {
                c.stderr(Stdio::null());
            }
            Some(Silence::Both) => {
                c.stdout(Stdio::null());
                c.stderr(Stdio::null());
            }
            None => {}
        }
    }
    c
}

fn resolve_unit_name(
    opts: &AppOpts,
    desktop: &str,
    app_name: Option<&str>,
    cmd: &[String],
) -> Result<String> {
    if let Some(u) = &opts.unit_name {
        let suffix = format!(".{}", opts.unit_type.as_str());
        if !u.ends_with(&suffix) {
            return Err(Error::InvalidArg(format!(
                "unit name must end with {suffix}"
            )));
        }
        if u.len() > 255 {
            return Err(Error::InvalidArg("unit name too long (> 255)".into()));
        }
        return Ok(u.clone());
    }
    let name = app_name
        .map(str::to_string)
        .unwrap_or_else(|| basename(&cmd[0]));
    Ok(naming::auto_unit_name(
        opts.unit_type.as_str(),
        desktop,
        &name,
        &random_hex8(),
    ))
}

fn resolve_slice(s: &str) -> Result<String> {
    Ok(match s {
        "a" => "app-graphical.slice".into(),
        "b" => "background-graphical.slice".into(),
        "s" => "session-graphical.slice".into(),
        other if other.ends_with(".slice") => other.to_string(),
        other => return Err(Error::InvalidArg(format!("invalid slice: {other:?}"))),
    })
}

fn final_description(desc: &Option<String>, app_name: Option<&str>, cmd: &[String]) -> String {
    desc.clone()
        .or_else(|| app_name.map(str::to_string))
        .unwrap_or_else(|| basename(&cmd[0]))
}

fn entry_description(entry: &DesktopEntry, action: Option<&str>) -> Option<String> {
    let name = entry.get_localized("Name", action).unwrap_or_default();
    let generic = entry.get_localized("GenericName", None).unwrap_or_default();
    let joined = [name, generic]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" - ");
    (!joined.is_empty()).then_some(joined)
}

fn desktop_entry_env_description() -> Option<String> {
    let pick = |a: &str, b: &str| {
        std::env::var(a)
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| std::env::var(b).ok().filter(|s| !s.is_empty()))
            .unwrap_or_default()
    };
    let name = pick("DESKTOP_ENTRY_NAME_L", "DESKTOP_ENTRY_NAME");
    let generic = pick("DESKTOP_ENTRY_GENERICNAME_L", "DESKTOP_ENTRY_GENERICNAME");
    let joined = [name, generic]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" - ");
    (!joined.is_empty()).then_some(joined)
}

fn session_specific_env() -> Vec<(String, String)> {
    varnames::SESSION_SPECIFIC
        .iter()
        .filter_map(|k| {
            std::env::var(*k)
                .ok()
                .filter(|v| !v.is_empty())
                .map(|v| ((*k).to_string(), v))
        })
        .collect()
}

fn first_desktop() -> String {
    std::env::var("XDG_CURRENT_DESKTOP")
        .ok()
        .and_then(|s| s.split(':').next().map(str::to_string))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "wsmr".to_string())
}

fn basename(p: &str) -> String {
    p.rsplit('/').next().unwrap_or(p).to_string()
}

fn random_hex8() -> String {
    use std::io::Read;
    let mut buf = [0u8; 4];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut buf);
    } else {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        buf.copy_from_slice(&n.to_le_bytes());
    }
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(unit_type: UnitType, silent: Option<Silence>) -> LaunchSpec {
        LaunchSpec {
            cmdline: vec!["firefox".into(), "https://x".into()],
            slice: "app-graphical.slice".into(),
            unit_type,
            unit_name: "app-niri-firefox-deadbeef.scope".into(),
            description: "Web Browser".into(),
            properties: vec!["SourcePath=/x/firefox.desktop".into()],
            silent,
            workdir: None,
            session_setenv: vec![("XDG_VTNR".into(), "1".into())],
        }
    }

    #[test]
    fn scope_argv() {
        let a = build_argv(&spec(UnitType::Scope, None));
        assert_eq!(&a[0..3], &["systemd-run", "--user", "--scope"]);
        assert!(a.contains(&"--slice=app-graphical.slice".to_string()));
        assert!(a.contains(&"--unit=app-niri-firefox-deadbeef.scope".to_string()));
        assert!(a.contains(&"--property=SourcePath=/x/firefox.desktop".to_string()));
        assert!(a.contains(&"--same-dir".to_string()));
        // scope does not inject --setenv
        assert!(!a.iter().any(|s| s.starts_with("--setenv=")));
        let dd = a.iter().position(|s| s == "--").unwrap();
        assert_eq!(&a[dd + 1..], &["firefox", "https://x"]);
    }

    #[test]
    fn service_argv_has_type_and_setenv() {
        let a = build_argv(&spec(UnitType::Service, Some(Silence::Both)));
        assert!(a.contains(&"--property=Type=exec".to_string()));
        assert!(a.contains(&"--property=ExitType=cgroup".to_string()));
        assert!(a.contains(&"--setenv=XDG_VTNR=1".to_string()));
        assert!(a.contains(&"--property=StandardOutput=null".to_string()));
        assert!(a.contains(&"--property=StandardError=null".to_string()));
    }

    #[test]
    fn workdir_overrides_same_dir() {
        let mut s = spec(UnitType::Scope, None);
        s.workdir = Some("/tmp".into());
        let a = build_argv(&s);
        assert!(a.contains(&"--working-directory=/tmp".to_string()));
        assert!(!a.contains(&"--same-dir".to_string()));
    }

    #[test]
    fn slice_resolution() {
        assert_eq!(resolve_slice("a").unwrap(), "app-graphical.slice");
        assert_eq!(resolve_slice("b").unwrap(), "background-graphical.slice");
        assert_eq!(resolve_slice("s").unwrap(), "session-graphical.slice");
        assert_eq!(resolve_slice("custom.slice").unwrap(), "custom.slice");
        assert!(resolve_slice("bogus").is_err());
    }
}
