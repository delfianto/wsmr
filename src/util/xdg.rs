//! Minimal XDG base-directory resolution (std::env only) — the subset of pyxdg
//! `BaseDirectory` that wsmr needs. See `REFERENCE.md` §11.

use crate::error::{Error, Result};
use std::path::PathBuf;

/// `$XDG_CONFIG_HOME`, falling back to `$HOME/.config`.
pub fn config_home() -> Result<PathBuf> {
    if let Some(v) = non_empty_var("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(v));
    }
    let home = non_empty_var("HOME").ok_or_else(|| Error::EnvMissing("HOME".into()))?;
    Ok(PathBuf::from(home).join(".config"))
}

/// `$XDG_RUNTIME_DIR`. No portable fallback — it is always set inside a logind
/// user session, which is the only place wsmr runs for real.
pub fn runtime_dir() -> Result<PathBuf> {
    non_empty_var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| Error::EnvMissing("XDG_RUNTIME_DIR".into()))
}

/// `$XDG_DATA_HOME`, falling back to `$HOME/.local/share`.
pub fn data_home() -> PathBuf {
    if let Some(v) = non_empty_var("XDG_DATA_HOME") {
        return PathBuf::from(v);
    }
    match std::env::var("HOME") {
        Ok(h) => PathBuf::from(h).join(".local/share"),
        Err(_) => PathBuf::from(".local/share"),
    }
}

/// `$XDG_DATA_DIRS`, falling back to `/usr/local/share:/usr/share`.
pub fn data_dirs() -> Vec<PathBuf> {
    non_empty_var("XDG_DATA_DIRS")
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_string())
        .split(':')
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect()
}

/// Full data hierarchy in search order: `data_home` first, then `data_dirs`.
pub fn data_paths() -> Vec<PathBuf> {
    let mut v = vec![data_home()];
    v.extend(data_dirs());
    v
}

/// `$XDG_CONFIG_DIRS`, falling back to `/etc/xdg`.
pub fn config_dirs() -> Vec<PathBuf> {
    non_empty_var("XDG_CONFIG_DIRS")
        .unwrap_or_else(|| "/etc/xdg".to_string())
        .split(':')
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect()
}

/// Full config hierarchy in search order: `config_home` first, then `config_dirs`.
pub fn config_paths() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Ok(h) = config_home() {
        v.push(h);
    }
    v.extend(config_dirs());
    v
}

fn non_empty_var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}
