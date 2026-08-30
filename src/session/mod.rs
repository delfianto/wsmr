//! Session orchestration: environment preparation, finalization, cleanup, the
//! readiness wait/exec machinery, and the `start` exec-chain.
//!
//! Runtime behavior is Linux-only and is exercised by the Tier B integration
//! suite. A small non-Linux fallback exists only for the pidfd syscall path.

pub mod check;
pub mod cleanup;
pub mod exec;
pub mod finalize;
pub mod helpers;
pub mod prepare;
pub mod start;
pub mod state;
pub mod stop;
pub mod wait;

use crate::error::Result;
use std::path::PathBuf;

/// Best-effort desktop notification for a user-facing failure. Detached
/// commands (finalize, app) have no visible stderr, so a notification is the
/// user's signal. Failures to notify are ignored.
pub fn notify_error(summary: &str, body: &str) {
    if let Ok(bus) = crate::sysd::dbus::SessionBus::connect() {
        let _ = bus.notify(summary, body);
    }
}

/// Best-effort: write `msg` to the journal under the `wsmr` identifier at
/// `err` priority, via a throwaway `systemd-cat` invocation.
///
/// `start()`'s pre-exec steps (system-target gate, double-start refusal,
/// unit-generation plan, bindpid start, login-env snapshot) run before its
/// own `systemd-cat`-wrapped hand-off to the signal handler, so an error
/// there only ever reaches plain stderr. A greetd-launched session has no
/// journal-captured stderr, so such a failure was otherwise completely
/// silent — the session just closes and greetd falls back to the greeter,
/// with no trace anywhere. Failures to log are ignored: this is a
/// best-effort diagnostic aid, not part of the session's correctness.
pub fn log_error_to_journal(msg: &str) {
    log_to_journal(msg, "err");
}

/// Same as [`log_error_to_journal`] but at `notice` priority, for
/// non-fatal, informational pre-exec messages (e.g. `start()` silently
/// adopting stale foreign drop-ins — see
/// `crate::units::plan::GenerationPlan::reclaimed`) that would otherwise be
/// just as invisible under a greetd-launched session as an actual error.
pub fn log_notice_to_journal(msg: &str) {
    log_to_journal(msg, "notice");
}

fn log_to_journal(msg: &str, priority: &str) {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let Ok(mut child) = Command::new("systemd-cat")
        .args(["--identifier=wsmr", &format!("--priority={priority}")])
        .stdin(Stdio::piped())
        .spawn()
    else {
        return;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = writeln!(stdin, "{msg}");
    }
    let _ = child.wait();
}

/// Program name used for runtime paths and identifiers.
pub const BIN_NAME: &str = "wsmr";

/// Path to a file under `$XDG_RUNTIME_DIR/wsmr/`.
pub(crate) fn runtime_path(name: &str) -> Result<PathBuf> {
    Ok(crate::util::xdg::runtime_dir()?.join(BIN_NAME).join(name))
}
