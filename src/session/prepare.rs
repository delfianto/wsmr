//! `prepare-env`: deduce session identity, run the shell loader, compute the
//! environment delta, and push it to the activation environments.
//! Ports `prepare_env` (`main.py:2682`) + `prepare-env.sh`. See `REFERENCE.md`
//! §3.2/§3.3.

use crate::comp::CompGlobals;
use crate::env::{delta, dump, files};
use crate::error::{Error, Result};
use crate::session::{helpers, runtime_path};
use crate::sysd::dbus::{SessionBus, SystemBus};
use crate::varnames;
use std::collections::BTreeMap;
use std::io::Read;
use std::process::Command;

/// Prepare the activation environment for `comp`.
pub fn prepare_env(comp: &CompGlobals) -> Result<()> {
    let bus = SessionBus::connect()?;

    // login env snapshot saved by `start` (consumed once)
    let env_login_path = runtime_path("env_login")?;
    let mut env_login = files::load_env(&env_login_path)?;
    let have_login = !env_login.is_empty();
    let _ = std::fs::remove_file(&env_login_path);

    deduce_session(&mut env_login)?;

    // current systemd activation env — saved for restore on cleanup
    let env_pre = bus.systemd_vars()?;
    files::save_env(&runtime_path("env_pre")?, &env_pre, files::Sep::Nul)?;

    // env handed to the shell loader: systemd env overridden by login env
    let mut env_merged = env_pre.clone();
    env_merged.extend(env_login.iter().map(|(k, v)| (k.clone(), v.clone())));

    let mark = random_mark();
    let env_post = run_loader(comp, &env_merged, &mark, have_login)?;

    // delta + apply
    let changes = delta::compute_changes(&env_pre, &env_post);
    files::append_cleanup(
        &runtime_path("env_cleanup.list")?,
        changes.cleanup.iter().cloned(),
    )?;
    if !changes.set.is_empty() {
        bus.set_systemd_vars(&changes.set)?;
    }
    if !changes.unset.is_empty() {
        let names: Vec<String> = changes.unset.into_iter().collect();
        bus.unset_systemd_vars(&names)?;
    }
    Ok(())
}

/// Fill `XDG_VTNR`/`XDG_SESSION_ID`/`XDG_SEAT` if missing, via the foreground VT
/// and logind, and (re)write `env_session.conf`.
fn deduce_session(env_login: &mut BTreeMap<String, String>) -> Result<()> {
    let nonempty = |m: &BTreeMap<String, String>, k: &str| m.get(k).is_some_and(|v| !v.is_empty());
    if nonempty(env_login, "XDG_SEAT") && nonempty(env_login, "XDG_SESSION_ID") {
        return Ok(());
    }

    let vt = match env_login
        .get("XDG_VTNR")
        .and_then(|s| s.parse::<u32>().ok())
    {
        Some(v) => v,
        None => {
            let v = crate::util::read_fg_vt()?;
            env_login.insert("XDG_VTNR".into(), v.to_string());
            v
        }
    };

    let sysbus = SystemBus::connect()?;
    let (sid, seat) = sysbus
        .session_by_vt(vt)?
        .ok_or_else(|| Error::Resolve(format!("could not determine login session on VT {vt}")))?;
    env_login.insert("XDG_SESSION_ID".into(), sid);
    env_login.insert("XDG_SEAT".into(), seat);

    // TODO(M3 fallback): if no wayland-session-bindpid@*.service is active,
    // start one on the session leader PID (best-effort login-session bind).

    save_session_conf(env_login)
}

/// Write `env_session.conf` (the `session_specific` vars) for unit
/// `EnvironmentFile=`.
fn save_session_conf(env: &BTreeMap<String, String>) -> Result<()> {
    let sess: BTreeMap<String, String> = varnames::SESSION_SPECIFIC
        .iter()
        .filter_map(|k| {
            env.get(*k)
                .filter(|v| !v.is_empty())
                .map(|v| ((*k).to_string(), v.clone()))
        })
        .collect();
    files::save_env(
        &runtime_path("env_session.conf")?,
        &sess,
        files::Sep::Newline,
    )
}

/// Run `prepare-env.sh` with the merged env, returning the parsed `env -0` dump.
fn run_loader(
    comp: &CompGlobals,
    env_merged: &BTreeMap<String, String>,
    mark: &str,
    have_login: bool,
) -> Result<BTreeMap<String, String>> {
    let aux_path = runtime_path(&format!("vars_{mark}"))?;
    if let Some(p) = aux_path.parent() {
        std::fs::create_dir_all(p).map_err(|e| Error::io(p, e))?;
    }
    std::fs::write(&aux_path, aux_vars(comp, mark, have_login))
        .map_err(|e| Error::io(&aux_path, e))?;

    let script = helpers::extract("prepare-env.sh")?;
    let output = Command::new("/bin/sh")
        .arg(&script)
        .arg(&aux_path)
        .env_clear()
        .envs(env_merged)
        .output()
        .map_err(|e| Error::io("/bin/sh", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed = dump::parse_shell_dump(&stdout, mark)?;
    if !output.status.success() {
        return Err(Error::Resolve(format!(
            "env preloader shell exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(parsed.env)
}

/// Build the aux-vars file sourced by `prepare-env.sh`.
fn aux_vars(comp: &CompGlobals, mark: &str, have_login: bool) -> String {
    let first_desktop = comp.desktop_names.first().map(String::as_str).unwrap_or("");
    let lines = [
        "__SELF_NAME__=wsmr".to_string(),
        format!("__WM_ID__={}", sh_quote(&comp.id)),
        format!("__WM_ID_UNIT_STRING__={}", sh_quote(&comp.id_unit_string)),
        format!("__WM_BIN_ID__={}", sh_quote(&comp.bin_id)),
        format!(
            "__WM_DESKTOP_NAMES__={}",
            sh_quote(&comp.desktop_names.join(":"))
        ),
        format!("__WM_FIRST_DESKTOP_NAME__={}", sh_quote(first_desktop)),
        "__WM_DESKTOP_NAMES_EXCLUSIVE__=false".to_string(),
        format!(
            "__LOAD_PROFILE__={}",
            if have_login { "true" } else { "false" }
        ),
        // value is space+tab+newline (default IFS); the quote spans the newline,
        // matching uwsm's aux-vars trick.
        "__OIFS__=\" \t\n\"".to_string(),
        format!("__RANDOM_MARK__={mark}"),
        "IN_UWSM_ENV_PRELOADER=true".to_string(),
    ];
    lines.join("\n") + "\n"
}

/// Minimal POSIX shell quoting (mirrors Python `shlex.quote`).
fn sh_quote(s: &str) -> String {
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

/// 16-hex-char random mark for the env dump boundary.
fn random_mark() -> String {
    let mut buf = [0u8; 8];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut buf);
    } else {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        buf.copy_from_slice(&n.to_le_bytes());
    }
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_mark_is_16_hex() {
        let m = random_mark();
        assert_eq!(m.len(), 16);
        assert!(m.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn sh_quote_cases() {
        assert_eq!(sh_quote("sway"), "sway");
        assert_eq!(sh_quote(""), "''");
        assert_eq!(sh_quote("a b"), "'a b'");
        assert_eq!(sh_quote("a:b/c-d"), "a:b/c-d");
    }

    #[test]
    fn save_session_conf_writes_only_session_specific() {
        use crate::testutil::with_env;
        let rt = std::env::temp_dir().join(format!("wsmr-sconf-{}", std::process::id()));
        std::fs::create_dir_all(rt.join("wsmr")).unwrap();
        with_env(&[("XDG_RUNTIME_DIR", Some(rt.to_str().unwrap()))], || {
            let mut env = BTreeMap::new();
            env.insert("XDG_VTNR".to_string(), "1".to_string());
            env.insert("XDG_SEAT".to_string(), "seat0".to_string());
            env.insert("NOT_SESSION_VAR".to_string(), "x".to_string());
            save_session_conf(&env).unwrap();
            let conf = std::fs::read_to_string(rt.join("wsmr/env_session.conf")).unwrap();
            assert!(conf.contains("XDG_VTNR=1"));
            assert!(!conf.contains("NOT_SESSION_VAR"));
        });
        let _ = std::fs::remove_dir_all(&rt);
    }

    #[test]
    fn deduce_session_short_circuits_when_seat_and_id_present() {
        let mut env = BTreeMap::new();
        env.insert("XDG_SEAT".into(), "seat0".into());
        env.insert("XDG_SESSION_ID".into(), "3".into());
        // both present → returns Ok without touching the (absent) system bus
        assert!(deduce_session(&mut env).is_ok());
    }

    #[test]
    fn deduce_session_needs_logind_when_incomplete() {
        // VT known but no seat/id → must reach logind; off a real seat/bus this
        // surfaces an error rather than fabricating a session.
        let mut env = BTreeMap::new();
        env.insert("XDG_VTNR".into(), "1".into());
        assert!(deduce_session(&mut env).is_err());
    }

    #[test]
    fn aux_vars_has_required_markers() {
        let comp = CompGlobals {
            cmdline: vec!["sway".into()],
            id: "sway".into(),
            id_unit_string: "sway".into(),
            bin_name: "sway".into(),
            bin_id: "sway".into(),
            desktop_names: vec!["sway".into()],
            name: None,
            description: None,
        };
        let aux = aux_vars(&comp, "deadbeef", true);
        assert!(aux.contains("__RANDOM_MARK__=deadbeef"));
        assert!(aux.contains("__WM_BIN_ID__=sway"));
        assert!(aux.contains("__LOAD_PROFILE__=true"));
        assert!(aux.contains("IN_UWSM_ENV_PRELOADER=true"));
        // OIFS value carries an embedded newline inside the quotes
        assert!(aux.contains("__OIFS__=\" \t\n\""));
    }
}
