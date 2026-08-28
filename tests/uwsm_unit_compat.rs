//! Regression check: wsmr's generated static graph (`units::templates::GRAPH`)
//! against uwsm 0.26.7's real, package-shipped unit files (P6-02).
//!
//! Every string below was captured verbatim from
//! `/usr/lib/systemd/user/*` on a host with the real `uwsm` 0.26.7 package
//! installed (`pacman -Ql uwsm`) — not retyped from templates.rs, so a copy-paste
//! bug can't make this pass vacuously. wsmr renders these with its own
//! bin_name/bin_path (`wsmr`/`/usr/bin/wsmr`); uwsm's shipped copies naturally use
//! its own (`uwsm`/`/usr/bin/uwsm`), so the comparison substitutes wsmr's context
//! with uwsm's values to normalize that one intentional difference before comparing.
//!
//! If this ever fails, either uwsm's unit graph changed upstream (update the
//! reference strings below from a current install) or wsmr's `GRAPH` drifted from
//! it unintentionally (fix `units::templates::GRAPH`, or, if intentional, move that
//! unit's reference string into a documented-divergence section here instead of
//! deleting the check).

use wsmr::units::templates::{self, RenderCtx};

/// Context matching uwsm 0.26.7's own generated `ExecStart=`/etc. values,
/// so rendering wsmr's templates with it should reproduce uwsm's real output
/// byte-for-byte for every unit that's meant to be shared, unmodified graph.
fn uwsm_ctx() -> RenderCtx {
    RenderCtx {
        bin_name: "uwsm".into(),
        bin_path: "/usr/bin/uwsm".into(),
        waitpid_bin: "waitpid".into(),
    }
}

#[test]
fn graph_matches_real_uwsm_0_26_7_shipped_units() {
    let reference: &[(&str, &str)] = &[
        (
            "wayland-session-envelope@.target",
            r#"[Unit]
Description=Session envelope of %I Wayland compositor
Documentation=man:uwsm(1) man:systemd.special(7)
BindsTo=wayland-wm-env@%i.service wayland-wm@%i.service
Before=wayland-wm-env@%i.service wayland-wm@%i.service
PropagatesStopTo=wayland-wm@%i.service
Conflicts=wayland-session-shutdown.target
After=wayland-session-shutdown.target
StopWhenUnneeded=yes
"#,
        ),
        (
            "wayland-session-pre@.target",
            r#"[Unit]
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
        ),
        (
            "wayland-session@.target",
            r#"[Unit]
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
        ),
        (
            "wayland-session-xdg-autostart@.target",
            r#"[Unit]
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
        ),
        (
            "wayland-session-shutdown.target",
            r#"[Unit]
Description=Shutdown graphical session units
Documentation=man:uwsm(1) man:systemd.special(7)
DefaultDependencies=no
Conflicts=graphical-session-pre.target graphical-session.target xdg-desktop-autostart.target
After=graphical-session-pre.target graphical-session.target xdg-desktop-autostart.target
StopWhenUnneeded=yes
"#,
        ),
        (
            "wayland-wm@.service",
            r#"[Unit]
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
# should be issued by 'uwsm finalize' for example
Type=notify
NotifyAccess=all
ExecStart=/usr/bin/uwsm aux exec -- %I
Restart=no
EnvironmentFile=-%t/uwsm/env_session.conf
TimeoutStartSec=30
TimeoutStopSec=10
SyslogIdentifier=uwsm_%I
Slice=session.slice
"#,
        ),
        (
            "wayland-wm-env@.service",
            r#"[Unit]
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
ExecStart=/usr/bin/uwsm aux prepare-env -- "%I"
ExecStopPost=/usr/bin/uwsm aux cleanup-env
Restart=no
EnvironmentFile=-%t/uwsm/env_session.conf
SyslogIdentifier=uwsm_env-preloader
Slice=session.slice
"#,
        ),
        (
            "wayland-session-waitenv.service",
            r#"[Unit]
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
ExecStart=/usr/bin/uwsm aux waitenv
Restart=no
TimeoutStartSec=30
SyslogIdentifier=uwsm_waitenv
Slice=background.slice
"#,
        ),
        (
            "wayland-session-bindpid@.service",
            r#"[Unit]
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
ExecStart=/bin/sh -c "if command -v waitpid >/dev/null; then exec waitpid -e %i; else exec /usr/bin/uwsm aux waitpid %i; fi" waitpid
Restart=no
SyslogIdentifier=uwsm_bindpid
Slice=background.slice
"#,
        ),
        (
            "wayland-wm-app-daemon.service",
            r#"[Unit]
Description=Fast application argument generator
Documentation=man:uwsm(1)
PartOf=graphical-session.target
After=graphical-session.target
Conflicts=wayland-session-shutdown.target
Before=wayland-session-shutdown.target
CollectMode=inactive-or-failed
[Service]
Type=exec
ExecStart=/usr/bin/uwsm aux app-daemon
Restart=on-failure
RestartMode=direct
EnvironmentFile=-%t/uwsm/env_session.conf
SyslogIdentifier=uwsm_app-daemon
Slice=session.slice
"#,
        ),
        (
            "session-graphical.slice",
            r#"[Unit]
Description=User Graphical Session Application Slice
Documentation=man:systemd.special(7)
PartOf=graphical-session.target
After=graphical-session.target
Conflicts=wayland-session-shutdown.target
Before=wayland-session-shutdown.target
"#,
        ),
        (
            "app-graphical.slice",
            r#"[Unit]
Description=User Graphical Application Slice
Documentation=man:uwsm(1) man:systemd.special(7)
PartOf=graphical-session.target
After=graphical-session.target
Conflicts=wayland-session-shutdown.target
Before=wayland-session-shutdown.target
"#,
        ),
        (
            "background-graphical.slice",
            r#"[Unit]
Description=User Graphical Background Application Slice
Documentation=man:uwsm(1) man:systemd.special(7)
PartOf=graphical-session.target
After=graphical-session.target
Conflicts=wayland-session-shutdown.target
Before=wayland-session-shutdown.target
"#,
        ),
    ];

    let ctx = uwsm_ctx();
    for (name, expected) in reference {
        let unit = templates::GRAPH
            .iter()
            .find(|u| u.name == *name)
            .unwrap_or_else(|| panic!("units::templates::GRAPH has no unit named {name:?}"));
        let rendered = templates::render(unit.body, &ctx);
        assert_eq!(
            &rendered, expected,
            "wsmr's generated {name} no longer matches uwsm 0.26.7's real shipped unit"
        );
    }

    assert_eq!(
        templates::GRAPH.len(),
        reference.len(),
        "GRAPH has grown/shrunk relative to the reference set captured from uwsm 0.26.7"
    );
}
