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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::with_env;
    use std::path::PathBuf;

    #[test]
    fn config_home_uses_var_then_home_fallback() {
        with_env(&[("XDG_CONFIG_HOME", Some("/cfg"))], || {
            assert_eq!(config_home().unwrap(), PathBuf::from("/cfg"));
        });
        with_env(
            &[("XDG_CONFIG_HOME", None), ("HOME", Some("/home/u"))],
            || assert_eq!(config_home().unwrap(), PathBuf::from("/home/u/.config")),
        );
        // empty var is treated as unset
        with_env(
            &[("XDG_CONFIG_HOME", Some("")), ("HOME", Some("/home/u"))],
            || assert_eq!(config_home().unwrap(), PathBuf::from("/home/u/.config")),
        );
        with_env(&[("XDG_CONFIG_HOME", None), ("HOME", None)], || {
            assert!(config_home().is_err());
        });
    }

    #[test]
    fn runtime_dir_requires_var() {
        with_env(&[("XDG_RUNTIME_DIR", Some("/run/user/1000"))], || {
            assert_eq!(runtime_dir().unwrap(), PathBuf::from("/run/user/1000"));
        });
        with_env(&[("XDG_RUNTIME_DIR", None)], || {
            assert!(runtime_dir().is_err());
        });
    }

    #[test]
    fn data_home_var_then_fallback() {
        with_env(&[("XDG_DATA_HOME", Some("/data"))], || {
            assert_eq!(data_home(), PathBuf::from("/data"));
        });
        with_env(&[("XDG_DATA_HOME", None), ("HOME", Some("/h"))], || {
            assert_eq!(data_home(), PathBuf::from("/h/.local/share"))
        });
        with_env(&[("XDG_DATA_HOME", None), ("HOME", None)], || {
            assert_eq!(data_home(), PathBuf::from(".local/share"));
        });
    }

    #[test]
    fn data_dirs_split_and_default() {
        with_env(&[("XDG_DATA_DIRS", Some("/a:/b::/c"))], || {
            assert_eq!(
                data_dirs(),
                vec![
                    PathBuf::from("/a"),
                    PathBuf::from("/b"),
                    PathBuf::from("/c")
                ]
            );
        });
        with_env(&[("XDG_DATA_DIRS", None)], || {
            assert_eq!(
                data_dirs(),
                vec![
                    PathBuf::from("/usr/local/share"),
                    PathBuf::from("/usr/share")
                ]
            );
        });
    }

    #[test]
    fn config_dirs_split_and_default() {
        with_env(&[("XDG_CONFIG_DIRS", Some("/etc/xdg:/x"))], || {
            assert_eq!(
                config_dirs(),
                vec![PathBuf::from("/etc/xdg"), PathBuf::from("/x")]
            );
        });
        with_env(&[("XDG_CONFIG_DIRS", None)], || {
            assert_eq!(config_dirs(), vec![PathBuf::from("/etc/xdg")]);
        });
    }

    #[test]
    fn data_paths_is_home_then_dirs() {
        with_env(
            &[
                ("XDG_DATA_HOME", Some("/d")),
                ("XDG_DATA_DIRS", Some("/x:/y")),
            ],
            || {
                assert_eq!(
                    data_paths(),
                    vec![
                        PathBuf::from("/d"),
                        PathBuf::from("/x"),
                        PathBuf::from("/y")
                    ]
                );
            },
        );
    }

    #[test]
    fn config_paths_is_home_then_dirs() {
        with_env(
            &[
                ("XDG_CONFIG_HOME", Some("/c")),
                ("XDG_CONFIG_DIRS", Some("/e")),
            ],
            || {
                assert_eq!(
                    config_paths(),
                    vec![PathBuf::from("/c"), PathBuf::from("/e")]
                );
            },
        );
        // config_home failing (no HOME) → only config_dirs
        with_env(
            &[
                ("XDG_CONFIG_HOME", None),
                ("HOME", None),
                ("XDG_CONFIG_DIRS", Some("/e")),
            ],
            || assert_eq!(config_paths(), vec![PathBuf::from("/e")]),
        );
    }
}
