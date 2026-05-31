//! Readiness waiting: `waitenv` (poll the activation environment) and `waitpid`
//! (block on a PID via pidfd). Ports `waitenv`/`waitpid` (`main.py:4464`/`:4433`).
//! See `REFERENCE.md` §4.3/§9.

use crate::error::{Error, Result};
use crate::filter;
use crate::sysd::dbus::SessionBus;
use std::collections::BTreeSet;
use std::thread::sleep;
use std::time::{Duration, Instant};

/// Wait timeout, from `$UWSM_WAIT_VARNAMES_TIMEOUT` (default 30s).
pub fn wait_timeout() -> Duration {
    let secs = std::env::var("UWSM_WAIT_VARNAMES_TIMEOUT")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|n| *n >= 1)
        .unwrap_or(30);
    Duration::from_secs(secs)
}

/// Post-appearance settle delay, from `$UWSM_WAIT_VARNAMES_SETTLETIME` (0.2s).
pub fn settle_time() -> Duration {
    let secs = std::env::var("UWSM_WAIT_VARNAMES_SETTLETIME")
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|n| *n >= 0.0)
        .unwrap_or(0.2);
    Duration::from_secs_f64(secs)
}

/// Wait until all `varnames` appear in the systemd activation environment.
pub fn waitenv(bus: &SessionBus, varnames: &[String], timeout: Duration) -> Result<()> {
    let expected: BTreeSet<String> = varnames
        .iter()
        .filter(|n| filter::keep_name(n))
        .cloned()
        .collect();
    let start = Instant::now();
    loop {
        let have: BTreeSet<String> = bus.systemd_vars()?.into_keys().collect();
        if expected.is_subset(&have) {
            return Ok(());
        }
        if start.elapsed() >= timeout {
            let missing: Vec<&String> = expected.difference(&have).collect();
            return Err(Error::Resolve(format!(
                "timed out waiting for activation-env variables: {}",
                missing
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
        sleep(Duration::from_millis(500));
    }
}

/// Block until `pid` exits, via `pidfd_open(2)` + `poll(2)` (Linux only).
#[cfg(target_os = "linux")]
pub fn waitpid(pid: i32) -> Result<()> {
    // SAFETY: pidfd_open is a thin syscall; -1 signals failure (checked below).
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    if fd < 0 {
        let e = std::io::Error::last_os_error();
        // process already gone is success
        if e.raw_os_error() == Some(libc::ESRCH) {
            return Ok(());
        }
        return Err(Error::io("pidfd_open", e));
    }
    let fd = fd as libc::c_int;
    let mut pfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: poll on one valid fd; blocks until the pid's pidfd is readable.
    let rc = unsafe { libc::poll(&mut pfd, 1, -1) };
    // SAFETY: closing our own fd.
    unsafe {
        libc::close(fd);
    }
    if rc < 0 {
        return Err(Error::io("poll", std::io::Error::last_os_error()));
    }
    Ok(())
}

/// Non-Linux stub: pidfd is Linux-only.
#[cfg(not(target_os = "linux"))]
pub fn waitpid(_pid: i32) -> Result<()> {
    Err(Error::todo("M3", "waitpid (pidfd is Linux-only)"))
}
