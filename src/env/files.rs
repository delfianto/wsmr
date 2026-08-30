//! Environment snapshot serialization and the generation-tagged cleanup list.

use crate::error::{Error, Result};
use crate::filter;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Record separator for env files.
#[derive(Clone, Copy, Debug)]
pub enum Sep {
    /// NUL-separated (safe for arbitrary values) — `env_login`, `env_pre`.
    Nul,
    /// Newline-separated — only for the simple `env_session.conf` written for a
    /// systemd `EnvironmentFile=`.
    Newline,
}

impl Sep {
    fn ch(self) -> char {
        match self {
            Sep::Nul => '\0',
            Sep::Newline => '\n',
        }
    }
}

/// Serialize `env` as `KEY=VALUE` joined by `sep`, dropping invalid names.
pub fn serialize_env(env: &BTreeMap<String, String>, sep: Sep) -> String {
    let s = sep.ch();
    let mut out = String::new();
    let mut first = true;
    for (k, v) in env {
        if !filter::keep_name(k) {
            continue;
        }
        if !first {
            out.push(s);
        }
        out.push_str(k);
        out.push('=');
        out.push_str(v);
        first = false;
    }
    // newline form is a text file → terminate with a newline
    if matches!(sep, Sep::Newline) && !out.is_empty() {
        out.push('\n');
    }
    out
}

/// Write `env` to `path` atomically (temp file + rename), creating parent
/// directories as needed, so a reader never observes a truncated file.
pub fn save_env(path: &Path, env: &BTreeMap<String, String>, sep: Sep) -> Result<()> {
    crate::util::fsutil::atomic_write_path(path, &serialize_env(env, sep))
}

/// Parse NUL-separated `KEY=VALUE` data, dropping invalid names. Each chunk is
/// split on its **first** `=` (values may contain `=`).
pub fn parse_env_nul(data: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for chunk in data.split('\0') {
        if chunk.is_empty() {
            continue;
        }
        if let Some((k, v)) = chunk.split_once('=')
            && filter::keep_name(k)
        {
            map.insert(k.to_string(), v.to_string());
        }
    }
    map
}

/// Read a NUL-separated env file. Missing file → empty map.
pub fn load_env(path: &Path) -> Result<BTreeMap<String, String>> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(parse_env_nul(&s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(e) => Err(Error::io(path, e)),
    }
}

/// One cleanup-list entry: a variable name tagged with the session
/// generation that requested its cleanup, so a reader can tell entries left
/// over by a different (older) session apart from its own. See
/// `crate::session::state`, which owns locking and generation IDs — this
/// module only (de)serializes the file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CleanupEntry {
    /// Generation id that recorded this entry.
    pub generation: String,
    /// Variable name to consider for cleanup.
    pub name: String,
}

/// Read every entry in the cleanup-list file, of any generation. Missing
/// file → empty set. Malformed lines are ignored, not trusted.
pub fn read_cleanup_entries(path: &Path) -> Result<BTreeSet<CleanupEntry>> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(s
            .lines()
            .filter_map(|l| {
                let (generation, name) = l.split_once(' ')?;
                (!generation.is_empty() && filter::keep_name(name)).then(|| CleanupEntry {
                    generation: generation.to_string(),
                    name: name.to_string(),
                })
            })
            .collect()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeSet::new()),
        Err(e) => Err(Error::io(path, e)),
    }
}

/// Atomically replace the cleanup-list file with exactly `entries`.
pub fn write_cleanup_entries(path: &Path, entries: &BTreeSet<CleanupEntry>) -> Result<()> {
    let mut body = String::new();
    for e in entries {
        body.push_str(&e.generation);
        body.push(' ');
        body.push_str(&e.name);
        body.push('\n');
    }
    crate::util::fsutil::atomic_write_path(path, &body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn tmp() -> PathBuf {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("wsmr-env-{}-{}", std::process::id(), n))
    }

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn nul_round_trip_with_tricky_values() {
        let dir = tmp();
        let path = dir.join("env_login");
        // values containing '=' and newline must survive NUL form
        let env = map(&[("A", "1=2"), ("B", "line1\nline2"), ("C", "")]);
        save_env(&path, &env, Sep::Nul).unwrap();
        let back = load_env(&path).unwrap();
        assert_eq!(back, env);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn newline_form_is_terminated() {
        let env = map(&[("XDG_VTNR", "1")]);
        let s = serialize_env(&env, Sep::Newline);
        assert_eq!(s, "XDG_VTNR=1\n");
    }

    #[test]
    fn serialize_drops_invalid_names() {
        let env = map(&[("OK", "1"), ("1BAD", "x"), ("SHELL", "/bin/sh")]);
        let s = serialize_env(&env, Sep::Nul);
        assert_eq!(s, "OK=1"); // SHELL and 1BAD dropped
    }

    #[test]
    fn load_missing_is_empty() {
        assert!(load_env(&tmp().join("nope")).unwrap().is_empty());
    }

    #[test]
    fn cleanup_entries_round_trip_and_stay_generation_tagged() {
        let dir = tmp();
        let path = dir.join("env_cleanup.list");
        let entries: BTreeSet<CleanupEntry> = [
            ("gen1", "FOO"),
            ("gen1", "BAR"),
            ("gen2", "FOO"), // same name, different generation — kept distinct
        ]
        .into_iter()
        .map(|(g, n)| CleanupEntry {
            generation: g.to_string(),
            name: n.to_string(),
        })
        .collect();
        write_cleanup_entries(&path, &entries).unwrap();
        let back = read_cleanup_entries(&path).unwrap();
        assert_eq!(back, entries);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cleanup_entries_missing_file_is_empty() {
        assert!(
            read_cleanup_entries(&tmp().join("nope"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn cleanup_entries_ignore_malformed_lines() {
        let dir = tmp();
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("env_cleanup.list");
        std::fs::write(&path, "no-space-here\ngen1 OK\ngen2 1BAD\n").unwrap();
        let got = read_cleanup_entries(&path).unwrap();
        assert_eq!(got.len(), 1);
        assert!(got.contains(&CleanupEntry {
            generation: "gen1".into(),
            name: "OK".into(),
        }));
        std::fs::remove_dir_all(&dir).ok();
    }
}
