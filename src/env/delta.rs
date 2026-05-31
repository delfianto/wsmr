//! The environment-delta set-algebra, ported from `prepare_env` (`main.py:2877`).
//!
//! Pure logic — the heart of the bootstrap. Given the systemd activation
//! environment before prep (`env_pre`) and the shell loader's resulting
//! environment (`env_post`), compute what to set, unset, and record for cleanup,
//! applying the [`crate::varnames`] policy. See `REFERENCE.md` §3.2.

use crate::varnames;
use std::collections::{BTreeMap, BTreeSet};

/// Environment changes to apply to the activation environments.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct EnvChanges {
    /// Variables to set (`KEY=VALUE`).
    pub set: BTreeMap<String, String>,
    /// Variable names to unset.
    pub unset: BTreeSet<String>,
    /// Variable names to record for cleanup on stop.
    pub cleanup: BTreeSet<String>,
}

/// Compute the env changes from the pre/post snapshots.
///
/// - `set` = `(post − pre)` (by key+value), then force `always_export ∩ post`,
///   then drop `never_export ∪ always_unset`.
/// - `unset` = `((pre.keys − post.keys) ∪ always_unset) ∩ pre.keys`.
/// - `cleanup` = `set.keys − never_cleanup`.
pub fn compute_changes(
    env_pre: &BTreeMap<String, String>,
    env_post: &BTreeMap<String, String>,
) -> EnvChanges {
    let never_export = varnames::never_export();
    let always_unset = varnames::always_unset();
    let always_export = varnames::always_export();
    let never_cleanup = varnames::never_cleanup();

    // set = entries in post whose (key, value) differ from pre (new or changed)
    let mut set: BTreeMap<String, String> = BTreeMap::new();
    for (k, v) in env_post {
        if env_pre.get(k) != Some(v) {
            set.insert(k.clone(), v.clone());
        }
    }

    // force-export always_export vars present in post (unless excluded)
    for &var in &always_export {
        if !never_export.contains(var)
            && !always_unset.contains(var)
            && let Some(v) = env_post.get(var)
        {
            set.insert(var.to_string(), v.clone());
        }
    }

    // never export these
    for var in never_export.iter().chain(always_unset.iter()) {
        set.remove(*var);
    }

    // unset = ((pre.keys − post.keys) ∪ always_unset) ∩ pre.keys
    let mut unset: BTreeSet<String> = BTreeSet::new();
    for k in env_pre.keys() {
        if !env_post.contains_key(k) {
            unset.insert(k.clone());
        }
    }
    for &var in &always_unset {
        if env_pre.contains_key(var) {
            unset.insert(var.to_string());
        }
    }

    // cleanup = set.keys − never_cleanup
    let cleanup: BTreeSet<String> = set
        .keys()
        .filter(|k| !never_cleanup.contains(k.as_str()))
        .cloned()
        .collect();

    EnvChanges {
        set,
        unset,
        cleanup,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn new_var_is_exported() {
        let pre = map(&[]);
        let post = map(&[("FOO", "bar")]);
        let c = compute_changes(&pre, &post);
        assert_eq!(c.set.get("FOO").map(String::as_str), Some("bar"));
        assert!(c.cleanup.contains("FOO"));
    }

    #[test]
    fn unchanged_var_not_exported() {
        let pre = map(&[("FOO", "bar")]);
        let post = map(&[("FOO", "bar")]);
        let c = compute_changes(&pre, &post);
        assert!(!c.set.contains_key("FOO"));
    }

    #[test]
    fn changed_var_is_exported() {
        let pre = map(&[("FOO", "old")]);
        let post = map(&[("FOO", "new")]);
        let c = compute_changes(&pre, &post);
        assert_eq!(c.set.get("FOO").map(String::as_str), Some("new"));
    }

    #[test]
    fn never_export_excluded_even_if_changed() {
        // SHELL is in never_export
        let pre = map(&[]);
        let post = map(&[("SHELL", "/bin/zsh"), ("OK", "1")]);
        let c = compute_changes(&pre, &post);
        assert!(!c.set.contains_key("SHELL"));
        assert!(c.set.contains_key("OK"));
    }

    #[test]
    fn always_unset_excluded_from_set_and_marked_unset() {
        // WAYLAND_DISPLAY is in always_unset; present in both pre and post
        let pre = map(&[("WAYLAND_DISPLAY", "wayland-0")]);
        let post = map(&[("WAYLAND_DISPLAY", "wayland-1")]);
        let c = compute_changes(&pre, &post);
        assert!(!c.set.contains_key("WAYLAND_DISPLAY"));
        // it's in pre, so it gets unset
        assert!(c.unset.contains("WAYLAND_DISPLAY"));
    }

    #[test]
    fn always_export_forced_from_post() {
        // PATH is in always_export; same value in pre and post → still forced
        let pre = map(&[("PATH", "/usr/bin")]);
        let post = map(&[("PATH", "/usr/bin")]);
        let c = compute_changes(&pre, &post);
        assert_eq!(c.set.get("PATH").map(String::as_str), Some("/usr/bin"));
    }

    #[test]
    fn unset_for_var_removed_in_post() {
        let pre = map(&[("GONE", "x"), ("KEPT", "y")]);
        let post = map(&[("KEPT", "y")]);
        let c = compute_changes(&pre, &post);
        assert!(c.unset.contains("GONE"));
        assert!(!c.unset.contains("KEPT"));
    }

    #[test]
    fn always_unset_only_when_in_pre() {
        // DISPLAY in always_unset but absent from pre → not unset
        let pre = map(&[]);
        let post = map(&[]);
        let c = compute_changes(&pre, &post);
        assert!(!c.unset.contains("DISPLAY"));
    }

    #[test]
    fn cleanup_excludes_never_cleanup() {
        // SSH_AUTH_SOCK is in never_cleanup
        let pre = map(&[]);
        let post = map(&[("SSH_AUTH_SOCK", "/run/x"), ("FOO", "1")]);
        let c = compute_changes(&pre, &post);
        assert!(c.set.contains_key("SSH_AUTH_SOCK")); // still exported
        assert!(!c.cleanup.contains("SSH_AUTH_SOCK")); // but not cleaned up
        assert!(c.cleanup.contains("FOO"));
    }
}
