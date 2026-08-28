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
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicPtr, Ordering};
use std::time::{Duration, Instant};

// Leaked CStrings of the two FIFO paths, set once at startup so the (async-
// signal-safe) handler can unlink them on a termination signal.
static IN_FIFO: AtomicPtr<libc::c_char> = AtomicPtr::new(std::ptr::null_mut());
static OUT_FIFO: AtomicPtr<libc::c_char> = AtomicPtr::new(std::ptr::null_mut());

extern "C" fn on_term_signal(_sig: libc::c_int) {
    // SAFETY: a signal handler may only call async-signal-safe functions —
    // `unlink` and `_exit` qualify. The pointers are set once at startup to
    // leaked CStrings that are never freed, so they stay valid here.
    unsafe {
        let inp = IN_FIFO.load(Ordering::Relaxed);
        if !inp.is_null() {
            libc::unlink(inp as *const libc::c_char);
        }
        let outp = OUT_FIFO.load(Ordering::Relaxed);
        if !outp.is_null() {
            libc::unlink(outp as *const libc::c_char);
        }
        libc::_exit(0);
    }
}

/// Trap SIGTERM/SIGINT/SIGHUP so the daemon removes its FIFOs and exits cleanly
/// (systemd sends SIGTERM on stop). Best-effort; failures to register are
/// non-fatal (the default disposition still terminates us).
fn install_signal_handlers(in_path: &Path, out_path: &Path) {
    if let Ok(c) = std::ffi::CString::new(in_path.as_os_str().as_bytes()) {
        IN_FIFO.store(c.into_raw(), Ordering::Relaxed);
    }
    if let Ok(c) = std::ffi::CString::new(out_path.as_os_str().as_bytes()) {
        OUT_FIFO.store(c.into_raw(), Ordering::Relaxed);
    }
    // SAFETY: registering a termination-signal handler whose body is
    // async-signal-safe (see `on_term_signal`).
    unsafe {
        for sig in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
            libc::signal(sig, on_term_signal as *const () as libc::sighandler_t);
        }
    }
}

fn remove_fifos(in_path: &Path, out_path: &Path) {
    let _ = std::fs::remove_file(in_path);
    let _ = std::fs::remove_file(out_path);
}

/// Run the app-daemon loop (until a `stop` command or a termination signal).
pub fn run() -> Result<()> {
    eprintln!("wsmr: launching app daemon");
    let in_path = create_fifo("wsmr-app-daemon-in")?;
    let out_path = create_fifo("wsmr-app-daemon-out")?;
    install_signal_handlers(&in_path, &out_path);
    loop {
        let in_path = create_fifo("wsmr-app-daemon-in")?;
        let out_path = create_fifo("wsmr-app-daemon-out")?;

        let line = std::fs::read_to_string(&in_path).map_err(|e| Error::io(&in_path, e))?;
        let args: Vec<String> = line
            .split('\0')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();

        match args.first().map(String::as_str) {
            None => send_reply("error 'No args given!' 2"),
            Some("stop") => {
                // Don't write a reply: the out-FIFO write would block waiting for
                // a reader the stopping client need not provide. Just clean up.
                eprintln!("wsmr: app daemon stopping");
                remove_fifos(&in_path, &out_path);
                return Ok(());
            }
            Some("ping") => send_reply("pong"),
            Some("app") => match handle_app(&args) {
                Ok(out) => send_reply(&out),
                Err(e) => send_reply(&format!("error {} 1", shquote(&format!("Error: {e}")))),
            },
            Some(_) => send_reply(&format!(
                "error {} 2",
                shquote(&format!("Invalid arguments: {}", args.join(" ")))
            )),
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

/// Send a reply, logging (not propagating) a failure — a client that gave up
/// before reading its reply must not take the whole daemon loop down with it
/// (P5-04). Used by every reply site in [`run`]'s main loop.
fn send_reply(text: &str) {
    if let Err(e) = send(text) {
        eprintln!("wsmr: app daemon: failed to send reply: {e}");
    }
}

/// How long [`send`] waits for a reader before giving up (P5-04).
const SEND_TIMEOUT: Duration = Duration::from_secs(5);
const SEND_POLL_INTERVAL: Duration = Duration::from_millis(20);

fn send(text: &str) -> Result<()> {
    let out = create_fifo("wsmr-app-daemon-out")?;
    let mut f = open_fifo_for_write_bounded(&out, SEND_TIMEOUT)?;
    f.write_all(format!("{text}\n").as_bytes())
        .map_err(|e| Error::io(&out, e))
}

/// Open a FIFO for writing without blocking forever if no reader ever shows
/// up: opening a FIFO for writing normally blocks until a reader opens the
/// other end, so a client that gave up (or crashed) between sending its
/// request and reading the reply would otherwise wedge the *entire* daemon
/// loop on a write nobody will ever read. Retries a non-blocking open
/// (`O_NONBLOCK` open fails immediately with `ENXIO` rather than blocking
/// when there's no reader yet) until `timeout` elapses, then clears
/// `O_NONBLOCK` before returning so the actual write behaves normally (safe
/// once a reader is confirmed present).
fn open_fifo_for_write_bounded(path: &Path, timeout: Duration) -> Result<std::fs::File> {
    let start = Instant::now();
    loop {
        match std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)
        {
            Ok(f) => {
                // SAFETY: `f`'s fd is valid for the duration of this call;
                // clearing O_NONBLOCK only affects blocking behavior, not
                // validity.
                unsafe {
                    libc::fcntl(std::os::unix::io::AsRawFd::as_raw_fd(&f), libc::F_SETFL, 0);
                }
                return Ok(f);
            }
            Err(e) if e.raw_os_error() == Some(libc::ENXIO) => {
                if start.elapsed() >= timeout {
                    return Err(Error::io(
                        path,
                        std::io::Error::other(
                            "timed out waiting for a reader on the app-daemon output FIFO",
                        ),
                    ));
                }
                std::thread::sleep(SEND_POLL_INTERVAL);
            }
            Err(e) => return Err(Error::io(path, e)),
        }
    }
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
        std::fs::write(
            &path,
            "[Desktop Entry]\nType=Application\nName=V\nExec=sh %f\n",
        )
        .unwrap();
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

    fn mkfifo(path: &Path) {
        let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        // SAFETY: mkfifo on a valid, NUL-terminated C string path.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    }

    /// P5-04: with no reader ever showing up, the bounded open must return
    /// (an error) within roughly `timeout` — not hang forever like a plain
    /// blocking `OpenOptions::write(true).open()` on a FIFO would.
    #[test]
    fn open_fifo_for_write_bounded_times_out_deterministically() {
        let dir = std::env::temp_dir().join(format!("wsmr-fifo-timeout-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out");
        mkfifo(&path);

        let start = Instant::now();
        let err = open_fifo_for_write_bounded(&path, Duration::from_millis(150)).unwrap_err();
        let elapsed = start.elapsed();
        assert!(err.to_string().contains("timed out"), "got: {err}");
        assert!(
            elapsed >= Duration::from_millis(150),
            "returned too early: {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "took far longer than the timeout: {elapsed:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The success path: a reader that opens shortly after the write attempt
    /// starts is picked up within the timeout, not treated as "no reader".
    #[test]
    fn open_fifo_for_write_bounded_succeeds_once_a_reader_appears() {
        let dir = std::env::temp_dir().join(format!("wsmr-fifo-ok-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out");
        mkfifo(&path);

        let reader_path = path.clone();
        let reader = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            std::fs::read_to_string(&reader_path).unwrap()
        });

        let mut f = open_fifo_for_write_bounded(&path, Duration::from_secs(2)).unwrap();
        f.write_all(b"hello\n").unwrap();
        drop(f);

        assert_eq!(reader.join().unwrap(), "hello\n");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
