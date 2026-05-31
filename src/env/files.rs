//! Environment file (de)serialization and the cleanup-list file.
//! Ports `save_env`/`load_env` (`main.py:2519`/`:2541`) and
//! `append_to_cleanup_file` (`main.py:2361`). See `REFERENCE.md` §3.1/§11.

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

/// Write `env` to `path`, creating parent directories.
pub fn save_env(path: &Path, env: &BTreeMap<String, String>, sep: Sep) -> Result<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).map_err(|e| Error::io(p, e))?;
    }
    std::fs::write(path, serialize_env(env, sep)).map_err(|e| Error::io(path, e))
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

/// Read the cleanup-list file into a set. Missing file → empty set.
pub fn read_cleanup(path: &Path) -> Result<BTreeSet<String>> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(s
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeSet::new()),
        Err(e) => Err(Error::io(path, e)),
    }
}

/// Merge `names` into the cleanup-list file (deduped, name-filtered, sorted).
/// No-op write when nothing new is added.
pub fn append_cleanup(path: &Path, names: impl IntoIterator<Item = String>) -> Result<()> {
    let mut existing = read_cleanup(path)?;
    let mut added = false;
    for n in names {
        if filter::keep_name(&n) && existing.insert(n) {
            added = true;
        }
    }
    if !added {
        return Ok(());
    }
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).map_err(|e| Error::io(p, e))?;
    }
    let body = existing.into_iter().collect::<Vec<_>>().join("\n");
    std::fs::write(path, format!("{body}\n")).map_err(|e| Error::io(path, e))
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
    fn cleanup_append_dedups_and_filters() {
        let dir = tmp();
        let path = dir.join("env_cleanup.list");
        append_cleanup(&path, ["FOO".into(), "BAR".into(), "1BAD".into()]).unwrap();
        // adding an existing one + a new one
        append_cleanup(&path, ["FOO".into(), "BAZ".into()]).unwrap();
        let got = read_cleanup(&path).unwrap();
        assert_eq!(
            got,
            ["FOO", "BAR", "BAZ"]
                .iter()
                .map(|s| s.to_string())
                .collect()
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
