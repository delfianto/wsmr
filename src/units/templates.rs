//! The systemd unit graph (embedded verbatim from upstream `systemd/user/*`)
//! plus per-compositor `50_custom.conf` drop-in generation
//! (ports `generate_dropins`, `main.py:1389`). See `REFERENCE.md` §14.
//!
//! Static units are kept byte-identical to upstream so they can be diffed
//! against the reference. Service units carry `@BIN_PATH@` / `@BIN_NAME@` /
//! `@WAITPID_BIN@` placeholders substituted by [`render`].

/// An installed unit file: its name and (possibly templated) body.
pub struct UnitTemplate {
    /// Installed file name (relative to the systemd user dir).
    pub name: &'static str,
    /// File body, possibly containing `@BIN_*@` placeholders.
    pub body: &'static str,
}

/// Substitution context for [`render`].
pub struct RenderCtx {
    /// Program name (e.g. `wsmr`) — used in `SyslogIdentifier`, `EnvironmentFile`.
    pub bin_name: String,
    /// Absolute path to the wsmr executable — used in `ExecStart`.
    pub bin_path: String,
    /// Name of the external `waitpid` helper (e.g. `waitpid`).
    pub waitpid_bin: String,
}

/// Substitute `@BIN_PATH@`, `@BIN_NAME@`, `@WAITPID_BIN@` in a unit body.
pub fn render(body: &str, ctx: &RenderCtx) -> String {
    body.replace("@BIN_PATH@", &ctx.bin_path)
        .replace("@WAITPID_BIN@", &ctx.waitpid_bin)
        .replace("@BIN_NAME@", &ctx.bin_name)
}

/// The full static unit graph.
pub const GRAPH: &[UnitTemplate] = &[
    UnitTemplate {
        name: "wayland-session-envelope@.target",
        body: r#"[Unit]
Description=Session envelope of %I Wayland compositor
Documentation=man:uwsm(1) man:systemd.special(7)
BindsTo=wayland-wm-env@%i.service wayland-wm@%i.service
Before=wayland-wm-env@%i.service wayland-wm@%i.service
PropagatesStopTo=wayland-wm@%i.service
Conflicts=wayland-session-shutdown.target
After=wayland-session-shutdown.target
StopWhenUnneeded=yes
"#,
    },
    UnitTemplate {
        name: "wayland-session-pre@.target",
        body: r#"[Unit]
Description=Preparation for session of %I Wayland compositor
Documentation=man:uwsm(1) man:systemd.special(7)
Requires=wayland-wm-env@%i.service
BindsTo=graphical-session-pre.target
Before=graphical-session-pre.target
PropagatesStopTo=graphical-session-pre.target
Conflicts=wayland-session-shutdown.target
Before=wayland-session-shutdown.target
RefuseManualStart=yes
RefuseManualStop=yes
StopWhenUnneeded=yes
"#,
    },
    UnitTemplate {
        name: "wayland-session@.target",
        body: r#"[Unit]
Description=Session of %I Wayland compositor
Documentation=man:uwsm(1) man:systemd.special(7)
Requires=wayland-session-pre@%i.target wayland-wm@%i.service
Wants=wayland-session-waitenv.service wayland-session-xdg-autostart@%i.target
After=graphical-session-pre.target
BindsTo=graphical-session.target
Before=graphical-session.target
PropagatesStopTo=graphical-session.target
Conflicts=wayland-session-shutdown.target
Before=wayland-session-shutdown.target
StopWhenUnneeded=yes
"#,
    },
    UnitTemplate {
        name: "wayland-session-xdg-autostart@.target",
        body: r#"[Unit]
Description=XDG Autostart for session of %I Wayland compositor
Documentation=man:uwsm(1) man:systemd.special(7)
PartOf=graphical-session.target
After=wayland-session@%i.target graphical-session.target
BindsTo=xdg-desktop-autostart.target
Before=xdg-desktop-autostart.target
PropagatesStopTo=xdg-desktop-autostart.target
Conflicts=wayland-session-shutdown.target
Before=wayland-session-shutdown.target
StopWhenUnneeded=yes
"#,
    },
    UnitTemplate {
        name: "wayland-session-shutdown.target",
        body: r#"[Unit]
Description=Shutdown graphical session units
Documentation=man:uwsm(1) man:systemd.special(7)
DefaultDependencies=no
Conflicts=graphical-session-pre.target graphical-session.target xdg-desktop-autostart.target
After=graphical-session-pre.target graphical-session.target xdg-desktop-autostart.target
StopWhenUnneeded=yes
"#,
    },
    UnitTemplate {
        name: "wayland-wm@.service",
        body: r#"[Unit]
Description=Main service for %I
Documentation=man:uwsm(1)
Requires=wayland-session-pre@%i.target
BindsTo=wayland-session@%i.target
Before=wayland-session@%i.target graphical-session.target
PropagatesStopTo=wayland-session@%i.target graphical-session.target
After=wayland-session-pre@%i.target graphical-session-pre.target
Wants=wayland-session-envelope@%i.target
OnSuccess=wayland-session-shutdown.target
OnSuccessJobMode=replace-irreversibly
OnFailure=wayland-session-shutdown.target
OnFailureJobMode=replace-irreversibly
Conflicts=wayland-session-shutdown.target
Before=wayland-session-shutdown.target
CollectMode=inactive-or-failed
[Service]
# awaits for ready state notification from compositor or child
# should be issued by '@BIN_NAME@ finalize' for example
Type=notify
NotifyAccess=all
ExecStart=@BIN_PATH@ aux exec -- %I
Restart=no
EnvironmentFile=-%t/@BIN_NAME@/env_session.conf
TimeoutStartSec=30
TimeoutStopSec=10
SyslogIdentifier=@BIN_NAME@_%I
Slice=session.slice
"#,
    },
    UnitTemplate {
        name: "wayland-wm-env@.service",
        body: r#"[Unit]
Description=Environment preloader for %I
Documentation=man:uwsm(1)
BindsTo=wayland-session-pre@%i.target
Before=wayland-session-pre@%i.target graphical-session-pre.target
PropagatesStopTo=wayland-session-pre@%i.target
Wants=wayland-session-envelope@%i.target
OnSuccess=wayland-session-shutdown.target
OnSuccessJobMode=replace-irreversibly
OnFailure=wayland-session-shutdown.target
OnFailureJobMode=replace-irreversibly
Conflicts=wayland-session-shutdown.target
Before=wayland-session-shutdown.target
RefuseManualStart=yes
RefuseManualStop=yes
StopWhenUnneeded=yes
CollectMode=inactive-or-failed
[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=@BIN_PATH@ aux prepare-env -- "%I"
ExecStopPost=@BIN_PATH@ aux cleanup-env
Restart=no
EnvironmentFile=-%t/@BIN_NAME@/env_session.conf
SyslogIdentifier=@BIN_NAME@_env-preloader
Slice=session.slice
"#,
    },
    UnitTemplate {
        name: "wayland-session-waitenv.service",
        body: r#"[Unit]
Description=Wait for WAYLAND_DISPLAY and other variables
Documentation=man:uwsm(1)
Before=graphical-session.target
After=graphical-session-pre.target
CollectMode=inactive-or-failed
OnFailure=wayland-session-shutdown.target
OnFailureJobMode=replace-irreversibly
Conflicts=wayland-session-shutdown.target
Before=wayland-session-shutdown.target
CollectMode=inactive-or-failed
[Service]
Type=oneshot
RemainAfterExit=no
ExecStart=@BIN_PATH@ aux waitenv
Restart=no
TimeoutStartSec=30
SyslogIdentifier=@BIN_NAME@_waitenv
Slice=background.slice
"#,
    },
    UnitTemplate {
        name: "wayland-session-bindpid@.service",
        body: r#"[Unit]
Description=Bind graphical session to PID %i
Documentation=man:uwsm(1)
OnSuccess=wayland-session-shutdown.target
OnSuccessJobMode=replace-irreversibly
OnFailure=wayland-session-shutdown.target
OnFailureJobMode=replace-irreversibly
Conflicts=wayland-session-shutdown.target
Before=wayland-session-shutdown.target
CollectMode=inactive-or-failed
[Service]
Type=exec
ExecStart=/bin/sh -c "if command -v @WAITPID_BIN@ >/dev/null; then exec @WAITPID_BIN@ -e %i; else exec @BIN_PATH@ aux waitpid %i; fi" waitpid
Restart=no
SyslogIdentifier=@BIN_NAME@_bindpid
Slice=background.slice
"#,
    },
    UnitTemplate {
        name: "wayland-wm-app-daemon.service",
        body: r#"[Unit]
Description=Fast application argument generator
Documentation=man:uwsm(1)
PartOf=graphical-session.target
After=graphical-session.target
Conflicts=wayland-session-shutdown.target
Before=wayland-session-shutdown.target
CollectMode=inactive-or-failed
[Service]
Type=exec
ExecStart=@BIN_PATH@ aux app-daemon
Restart=on-failure
RestartMode=direct
EnvironmentFile=-%t/@BIN_NAME@/env_session.conf
SyslogIdentifier=@BIN_NAME@_app-daemon
Slice=session.slice
"#,
    },
    UnitTemplate {
        name: "session-graphical.slice",
        body: r#"[Unit]
Description=User Graphical Session Application Slice
Documentation=man:systemd.special(7)
PartOf=graphical-session.target
After=graphical-session.target
Conflicts=wayland-session-shutdown.target
Before=wayland-session-shutdown.target
"#,
    },
    UnitTemplate {
        name: "app-graphical.slice",
        body: r#"[Unit]
Description=User Graphical Application Slice
Documentation=man:uwsm(1) man:systemd.special(7)
PartOf=graphical-session.target
After=graphical-session.target
Conflicts=wayland-session-shutdown.target
Before=wayland-session-shutdown.target
"#,
    },
    UnitTemplate {
        name: "background-graphical.slice",
        body: r#"[Unit]
Description=User Graphical Background Application Slice
Documentation=man:uwsm(1) man:systemd.special(7)
PartOf=graphical-session.target
After=graphical-session.target
Conflicts=wayland-session-shutdown.target
Before=wayland-session-shutdown.target
"#,
    },
];

/// Inputs to the per-compositor `50_custom.conf` drop-in generation.
#[derive(Debug, Default, Clone)]
pub struct DropinInput {
    /// Compositor id (basename) — goes into `X-UWSMMark`.
    pub id: String,
    /// systemd-escaped id — used for the `.d/` subdir paths.
    pub id_unit_string: String,
    /// Program path for the generated `ExecStart=`.
    pub bin_path: String,
    /// Compositor binary basename (fallback description / desktop-name compare).
    pub bin_name: String,
    /// Optional compositor display name.
    pub name: Option<String>,
    /// Optional compositor description.
    pub description: Option<String>,
    /// Final desktop-name list.
    pub desktop_names: Vec<String>,
    /// CLI-supplied desktop names.
    pub cli_desktop_names: Vec<String>,
    /// Whether the CLI desktop names are exclusive (`-e`).
    pub cli_desktop_names_exclusive: bool,
    /// Full resolved compositor command line.
    pub cmdline: Vec<String>,
    /// CLI-supplied compositor args (after arg0).
    pub cli_args: Vec<String>,
}

fn non_empty(opt: &Option<String>) -> Option<&str> {
    opt.as_deref().filter(|s| !s.is_empty())
}

fn desc_substring(input: &DropinInput) -> String {
    let first = non_empty(&input.name)
        .unwrap_or(&input.bin_name)
        .to_string();
    let mut parts = vec![first];
    if let Some(d) = non_empty(&input.description) {
        parts.push(d.to_string());
    }
    parts.join(", ")
}

fn first_is_absolute(input: &DropinInput) -> bool {
    input.cmdline.first().is_some_and(|a| a.starts_with('/'))
}

fn header(id: &str) -> String {
    format!("# injected by wsmr, do not edit\n[Unit]\nX-UWSMMark={id}\n")
}

fn exec_override(base: &[&str], extra: &[String]) -> String {
    let mut parts: Vec<String> = base.iter().map(|s| s.to_string()).collect();
    parts.extend(extra.iter().cloned());
    format!("[Service]\nExecStart=\nExecStart={}\n", shlex_join(&parts))
}

/// Generate the env-preloader drop-in, or `None` if no customization is needed.
pub fn preloader_dropin(input: &DropinInput) -> Option<String> {
    let mut extra = String::new();
    if non_empty(&input.name).is_some() || non_empty(&input.description).is_some() {
        extra.push_str(&format!(
            "Description=Environment preloader for {}\n",
            desc_substring(input)
        ));
    }

    let mut args: Vec<String> = Vec::new();
    if input.cli_desktop_names_exclusive {
        args.push("-eD".into());
        args.push(input.cli_desktop_names.join(":"));
    } else if input.desktop_names != [input.bin_name.clone()] {
        args.push("-D".into());
        args.push(input.desktop_names.join(":"));
    }

    let abs = first_is_absolute(input);
    if !args.is_empty() || abs {
        args.push("--".into());
        args.push("%I".into());
        if abs {
            args.push(input.cmdline[0].clone());
        }
        extra.push_str(&exec_override(
            &[&input.bin_path, "aux", "prepare-env"],
            &args,
        ));
    }

    if extra.is_empty() {
        None
    } else {
        Some(format!("{}{}", header(&input.id), extra))
    }
}

/// Generate the main-service drop-in, or `None` if no customization is needed.
pub fn service_dropin(input: &DropinInput) -> Option<String> {
    let mut extra = String::new();
    if non_empty(&input.name).is_some() || non_empty(&input.description).is_some() {
        extra.push_str(&format!(
            "Description=Main service for {}\n",
            desc_substring(input)
        ));
    }

    let mut svc: Vec<String> = Vec::new();
    if first_is_absolute(input) {
        svc.extend(input.cmdline.iter().cloned());
    } else if !input.cli_args.is_empty() {
        svc.push(String::new());
        svc.extend(input.cli_args.iter().cloned());
    }

    if !svc.is_empty() {
        extra.push_str(&exec_override(
            &[&input.bin_path, "aux", "exec", "--", "%I"],
            &svc,
        ));
    }

    if extra.is_empty() {
        None
    } else {
        Some(format!("{}{}", header(&input.id), extra))
    }
}

/// Join arguments with POSIX shell quoting (mirrors Python `shlex.join`).
pub fn shlex_join(parts: &[String]) -> String {
    parts
        .iter()
        .map(|p| shlex_quote(p))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shlex_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || "@%+=:,./-_".contains(c))
    {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> RenderCtx {
        RenderCtx {
            bin_name: "wsmr".into(),
            bin_path: "/usr/bin/wsmr".into(),
            waitpid_bin: "waitpid".into(),
        }
    }

    #[test]
    fn graph_has_all_units() {
        assert_eq!(GRAPH.len(), 13);
        assert!(GRAPH.iter().any(|u| u.name == "wayland-wm@.service"));
        assert!(
            GRAPH
                .iter()
                .any(|u| u.name == "wayland-session-shutdown.target")
        );
    }

    #[test]
    fn render_substitutes_placeholders() {
        let wm = GRAPH
            .iter()
            .find(|u| u.name == "wayland-wm@.service")
            .unwrap();
        let out = render(wm.body, &ctx());
        assert!(out.contains("ExecStart=/usr/bin/wsmr aux exec -- %I"));
        assert!(out.contains("SyslogIdentifier=wsmr_%I"));
        assert!(out.contains("EnvironmentFile=-%t/wsmr/env_session.conf"));
        assert!(!out.contains("@BIN_"));
    }

    #[test]
    fn static_targets_have_no_placeholders() {
        let t = GRAPH
            .iter()
            .find(|u| u.name == "wayland-session-shutdown.target")
            .unwrap();
        assert_eq!(render(t.body, &ctx()), t.body);
    }

    #[test]
    fn minimal_dropins_are_none() {
        // bare exec, no names/desc, desktop_names == [bin_name], not absolute
        let input = DropinInput {
            id: "sway".into(),
            id_unit_string: "sway".into(),
            bin_path: "/usr/bin/wsmr".into(),
            bin_name: "sway".into(),
            desktop_names: vec!["sway".into()],
            cmdline: vec!["sway".into()],
            ..Default::default()
        };
        assert!(preloader_dropin(&input).is_none());
        assert!(service_dropin(&input).is_none());
    }

    #[test]
    fn name_adds_description_to_both() {
        let input = DropinInput {
            id: "sway".into(),
            bin_name: "sway".into(),
            bin_path: "/usr/bin/wsmr".into(),
            desktop_names: vec!["sway".into()],
            cmdline: vec!["sway".into()],
            name: Some("My Sway".into()),
            ..Default::default()
        };
        let pre = preloader_dropin(&input).unwrap();
        let svc = service_dropin(&input).unwrap();
        assert!(pre.contains("X-UWSMMark=sway"));
        assert!(pre.contains("Description=Environment preloader for My Sway"));
        assert!(svc.contains("Description=Main service for My Sway"));
    }

    #[test]
    fn absolute_path_hardcodes_service_exec() {
        let input = DropinInput {
            id: "sway".into(),
            bin_path: "/usr/bin/wsmr".into(),
            bin_name: "sway".into(),
            desktop_names: vec!["sway".into()],
            cmdline: vec!["/usr/bin/sway".into(), "--unsupported-gpu".into()],
            ..Default::default()
        };
        let svc = service_dropin(&input).unwrap();
        assert!(
            svc.contains("ExecStart=/usr/bin/wsmr aux exec -- %I /usr/bin/sway --unsupported-gpu")
        );
        // absolute path also forces the preloader `-- %I <path>` form
        let pre = preloader_dropin(&input).unwrap();
        assert!(pre.contains("aux prepare-env -- %I /usr/bin/sway"));
    }

    #[test]
    fn differing_desktop_names_add_dash_d() {
        let input = DropinInput {
            id: "sway".into(),
            bin_path: "/usr/bin/wsmr".into(),
            bin_name: "sway".into(),
            desktop_names: vec!["Hyprland".into(), "wlroots".into()],
            cmdline: vec!["sway".into()],
            ..Default::default()
        };
        let pre = preloader_dropin(&input).unwrap();
        assert!(pre.contains("aux prepare-env -D Hyprland:wlroots -- %I"));
    }

    #[test]
    fn shlex_join_quotes() {
        assert_eq!(shlex_join(&["a".into(), "b c".into()]), "a 'b c'");
        assert_eq!(shlex_join(&["".into()]), "''");
        assert_eq!(shlex_join(&["/usr/bin/x".into()]), "/usr/bin/x");
    }
}
