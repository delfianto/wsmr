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

fn non_empty_var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}
