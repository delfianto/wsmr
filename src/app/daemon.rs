//! `aux app-daemon`: a long-running FIFO server that resolves `app` argument
//! lines into `systemd-run` shell commands, so a thin client can launch apps
//! without paying process startup each time. Ports `app_daemon`
//! (`main.py:3815`). See analysis §6.
//!
//! Protocol: the client writes a NUL-separated argv to `wsmr-app-daemon-in`; the
//! daemon writes one shell line to `wsmr-app-daemon-out`:
//! `pong` · `exec systemd-run …` · `… & … & wait` · `error '<msg>' <code>`.
//!
//! Signal trapping is omitted — systemd stops the unit; we exit on `stop` or a
//! default SIGTERM.

use crate::app::launch;
use crate::cli::{Cli, Command as CliCommand};
use crate::error::{Error, Result};
use crate::units::templates::shlex_join;
use crate::util::xdg;
use clap::Parser;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

/// Run the app-daemon loop (until a `stop` command).
pub fn run() -> Result<()> {
    eprintln!("wsmr: launching app daemon");
    loop {
        let in_path = create_fifo("wsmr-app-daemon-in")?;
        let _ = create_fifo("wsmr-app-daemon-out")?;

        let line = std::fs::read_to_string(&in_path).map_err(|e| Error::io(&in_path, e))?;
        let args: Vec<String> = line
            .split('\0')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();

        match args.first().map(String::as_str) {
            None => send("error 'No args given!' 2")?,
            Some("stop") => {
                send("message 'Stopping app daemon.'")?;
                return Ok(());
            }
            Some("ping") => send("pong")?,
            Some("app") => match handle_app(&args) {
                Ok(out) => send(&out)?,
                Err(e) => send(&format!("error {} 1", shquote(&format!("Error: {e}"))))?,
            },
            Some(_) => send(&format!(
                "error {} 2",
                shquote(&format!("Invalid arguments: {}", args.join(" ")))
            ))?,
        }
    }
}

/// Parse an `app …` argv and emit the shell command(s) to run it.
fn handle_app(args: &[String]) -> Result<String> {
    let cli = Cli::try_parse_from(std::iter::once("wsmr".to_string()).chain(args.iter().cloned()))
        .map_err(|e| Error::InvalidArg(format!("Invalid arguments: {e}")))?;
    let CliCommand::App(app_args) = cli.command else {
        return Err(Error::InvalidArg("not an app command".into()));
    };
    let argvs = launch::resolve(&app_args.into())?;
    if argvs.len() == 1 {
        Ok(format!("exec {}", shlex_join(&argvs[0])))
    } else {
        let mut parts: Vec<String> = argvs
            .iter()
            .map(|a| format!("{} &", shlex_join(a)))
            .collect();
        parts.push("wait".to_string());
        Ok(parts.join(" "))
    }
}

fn send(text: &str) -> Result<()> {
    let out = create_fifo("wsmr-app-daemon-out")?;
    std::fs::write(&out, format!("{text}\n")).map_err(|e| Error::io(&out, e))
}

fn shquote(s: &str) -> String {
    shlex_join(std::slice::from_ref(&s.to_string()))
}

/// Ensure `$XDG_RUNTIME_DIR/<name>` is a FIFO; create it if missing.
fn create_fifo(name: &str) -> Result<PathBuf> {
    let path = xdg::runtime_dir()?.join(name);
    if path.exists() {
        if is_fifo(&path) {
            return Ok(path);
        }
        std::fs::remove_file(&path).map_err(|e| Error::io(&path, e))?;
    }
    let cpath = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| Error::InvalidArg("path contains NUL".into()))?;
    // SAFETY: mkfifo on a valid C string path; return code is checked.
    let rc = unsafe { libc::mkfifo(cpath.as_ptr(), 0o600) };
    if rc != 0 {
        let e = std::io::Error::last_os_error();
        if e.raw_os_error() != Some(libc::EEXIST) {
            return Err(Error::io(&path, e));
        }
    }
    Ok(path)
}

fn is_fifo(p: &Path) -> bool {
    use std::os::unix::fs::FileTypeExt;
    std::fs::metadata(p)
        .map(|m| m.file_type().is_fifo())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_app_emits_exec_systemd_run() {
        // `sleep` is on PATH on the dev host; a scope needs no D-Bus to resolve.
        let args = vec!["app".into(), "--".into(), "sleep".into(), "600".into()];
        let out = handle_app(&args).unwrap();
        assert!(
            out.starts_with("exec systemd-run --user --scope"),
            "got: {out}"
        );
        assert!(out.ends_with("-- sleep 600"), "got: {out}");
    }

    #[test]
    fn handle_app_rejects_bad_args() {
        assert!(handle_app(&["app".into(), "--bogus-flag".into()]).is_err());
    }

    #[test]
    fn handle_app_multi_instance_uses_wait() {
        // a .desktop with %f + two files → multi-instance → "<a> & <b> & wait"
        let dir = std::env::temp_dir().join(format!("wsmr-daemon-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("v.desktop");
        std::fs::write(&path, "[Desktop Entry]\nType=Application\nExec=sh %f\n").unwrap();
        let out = handle_app(&[
            "app".into(),
            path.to_string_lossy().into_owned(),
            "/etc/hostname".into(),
            "/etc/hosts".into(),
        ])
        .unwrap();
        assert!(out.ends_with(" & wait"), "got: {out}");
        assert_eq!(out.matches(" & ").count(), 2, "two jobs + wait: {out}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shquote_quotes_spaces() {
        assert_eq!(shquote("a b"), "'a b'");
        assert_eq!(shquote("plain"), "plain");
    }

    #[test]
    fn create_fifo_makes_and_reuses_fifo() {
        use crate::testutil::with_env;
        let rt = std::env::temp_dir().join(format!("wsmr-fifo-{}", std::process::id()));
        std::fs::create_dir_all(&rt).unwrap();
        with_env(&[("XDG_RUNTIME_DIR", Some(rt.to_str().unwrap()))], || {
            let p = create_fifo("test-fifo").unwrap();
            assert!(is_fifo(&p));
            // idempotent: a second call returns the same existing FIFO
            let p2 = create_fifo("test-fifo").unwrap();
            assert_eq!(p, p2);
            assert!(is_fifo(&p2));

            // a pre-existing *regular* file at the path is replaced with a FIFO
            let plain = rt.join("plainfile");
            std::fs::write(&plain, "x").unwrap();
            assert!(!is_fifo(&plain));
            let p3 = create_fifo("plainfile").unwrap();
            assert!(is_fifo(&p3));
        });
        let _ = std::fs::remove_dir_all(&rt);
    }
}
