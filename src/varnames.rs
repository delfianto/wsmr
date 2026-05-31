//! Variable-name classification sets, ported from uwsm's `Varnames`
//! (`uwsm/uwsm/main.py:141`).
//!
//! These drive which environment variables get exported to / scrubbed from the
//! systemd and D-Bus activation environments during session bootstrap and
//! teardown. See `REFERENCE.md` §2.

use std::collections::BTreeSet;

/// Highly login-session-bound vars. Never pushed to the *shared* activation
/// environment; instead delivered per-unit via `EnvironmentFile=` / `--setenv`.
pub const SESSION_SPECIFIC: &[&str] = &[
    "XDG_SEAT",
    "XDG_SEAT_PATH",
    "XDG_SESSION_ID",
    "XDG_SESSION_PATH",
    "XDG_VTNR",
];

/// Force-exported even when unchanged during environment preparation.
pub const ALWAYS_EXPORT: &[&str] = &[
    "PATH",
    "XDG_CURRENT_DESKTOP",
    "XDG_MENU_PREFIX",
    "XDG_SESSION_DESKTOP",
    "XDG_SESSION_CLASS",
    "XDG_SESSION_TYPE",
];

/// Never exported (shell noise + identity/socket vars). [`never_export`] unions
/// this with [`SESSION_SPECIFIC`].
const NEVER_EXPORT_BASE: &[&str] = &[
    "PWD",
    "LS_COLORS",
    "INVOCATION_ID",
    "SHLVL",
    "SHELL",
    "TERM",
    "COLORTERM",
    "TERM_SESSION_TYPE",
    "NOTIFY_SOCKET",
];

/// Always removed from the activation env during prep. [`always_unset`] unions
/// this with [`SESSION_SPECIFIC`].
const ALWAYS_UNSET_BASE: &[&str] = &["DISPLAY", "WAYLAND_DISPLAY"];

/// Always scrubbed on stop. [`always_cleanup`] unions this with
/// [`SESSION_SPECIFIC`].
const ALWAYS_CLEANUP_BASE: &[&str] = &[
    "DISPLAY",
    "LANG",
    "PATH",
    "WAYLAND_DISPLAY",
    "XCURSOR_SIZE",
    "XCURSOR_THEME",
    "XDG_CURRENT_DESKTOP",
    "XDG_MENU_PREFIX",
    "XDG_SESSION_DESKTOP",
    "XDG_SESSION_CLASS",
    "XDG_SESSION_TYPE",
    "NOTIFY_SOCKET",
];

/// Protected from cleanup (e.g. the SSH agent vars).
pub const NEVER_CLEANUP: &[&str] = &["SSH_AGENT_LAUNCHER", "SSH_AUTH_SOCK", "SSH_AGENT_PID"];

fn set_of(items: &[&'static str]) -> BTreeSet<&'static str> {
    items.iter().copied().collect()
}

fn union_session(base: &[&'static str]) -> BTreeSet<&'static str> {
    let mut s = set_of(base);
    s.extend(SESSION_SPECIFIC.iter().copied());
    s
}

/// `session_specific` set.
pub fn session_specific() -> BTreeSet<&'static str> {
    set_of(SESSION_SPECIFIC)
}

/// `always_export` set.
pub fn always_export() -> BTreeSet<&'static str> {
    set_of(ALWAYS_EXPORT)
}

/// `never_export` = base ∪ `session_specific`.
pub fn never_export() -> BTreeSet<&'static str> {
    union_session(NEVER_EXPORT_BASE)
}

/// `always_unset` = base ∪ `session_specific`.
pub fn always_unset() -> BTreeSet<&'static str> {
    union_session(ALWAYS_UNSET_BASE)
}

/// `always_cleanup` = base ∪ `session_specific`.
pub fn always_cleanup() -> BTreeSet<&'static str> {
    union_session(ALWAYS_CLEANUP_BASE)
}

/// `never_cleanup` set.
pub fn never_cleanup() -> BTreeSet<&'static str> {
    set_of(NEVER_CLEANUP)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_specific_members() {
        let s = session_specific();
        assert!(s.contains("XDG_VTNR"));
        assert!(s.contains("XDG_SESSION_ID"));
        assert_eq!(s.len(), 5);
    }

    #[test]
    fn never_export_includes_session_specific() {
        let s = never_export();
        assert!(s.contains("SHELL"));
        assert!(s.contains("NOTIFY_SOCKET"));
        // unioned in:
        assert!(s.contains("XDG_SEAT"));
    }

    #[test]
    fn always_unset_includes_session_specific() {
        let s = always_unset();
        assert!(s.contains("WAYLAND_DISPLAY"));
        assert!(s.contains("DISPLAY"));
        assert!(s.contains("XDG_VTNR"));
    }

    #[test]
    fn always_cleanup_superset_of_always_export_basics() {
        let c = always_cleanup();
        assert!(c.contains("PATH"));
        assert!(c.contains("XCURSOR_THEME"));
        assert!(c.contains("XDG_SESSION_ID")); // session_specific unioned
    }

    #[test]
    fn never_cleanup_protects_ssh() {
        let s = never_cleanup();
        assert!(s.contains("SSH_AUTH_SOCK"));
        assert!(!s.contains("PATH"));
    }
}
