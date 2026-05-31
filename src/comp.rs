//! Compositor argument classification and resolution.
//!
//! Ports `MainArg` (`main.py:188`) and the **bare-executable** branch of
//! `fill_comp_globals` (`main.py:3965`/`:4292`). Desktop-entry compositor
//! resolution is deferred to M3. See `docs/uwsm-core-analysis.md` §3.2/§6.

use crate::error::{Error, Result};
use crate::units::escape::simple_systemd_escape;
use std::collections::HashSet;
use std::path::PathBuf;

/// Classification of the main argument: a desktop entry, or an executable —
/// either possibly given as a filesystem path.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MainArg {
    /// Desktop entry id (e.g. `firefox.desktop`), if the arg is an entry.
    pub entry_id: Option<String>,
    /// Desktop action id following a `:` (e.g. `new-window`).
    pub entry_action: Option<String>,
    /// Executable name/command, if the arg is not an entry.
    pub executable: Option<String>,
    /// Normalized path, if the arg contained a `/`.
    pub path: Option<PathBuf>,
}

impl MainArg {
    /// Classify a raw argument string.
    pub fn parse(arg: &str) -> Result<MainArg> {
        let mut out = MainArg::default();

        if arg.ends_with(".desktop") || arg.contains(".desktop:") {
            let (id, action) = match arg.split_once(':') {
                Some((id, action)) if !action.is_empty() => {
                    if !is_action_id(action) {
                        return Err(Error::InvalidArg(format!(
                            "invalid Desktop Entry Action \"{action}\""
                        )));
                    }
                    (id.to_string(), Some(action.to_string()))
                }
                Some((id, _)) => (id.to_string(), None),
                None => (arg.to_string(), None),
            };

            let mut entry_id = id;
            if entry_id.contains('/') {
                // path to an entry: keep basename as id (data-dir relpath
                // refinement deferred to M5)
                let p = normalize_user_path(&entry_id);
                entry_id = p
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or(entry_id);
                out.path = Some(p);
            }

            if out.path.is_none() && !is_entry_id(&entry_id) {
                return Err(Error::InvalidArg(format!(
                    "invalid Desktop Entry ID \"{entry_id}\""
                )));
            }
            out.entry_id = Some(entry_id);
            out.entry_action = action;
        } else {
            out.executable = Some(arg.to_string());
            if arg.contains('/') {
                out.path = Some(normalize_user_path(arg));
            }
        }
        Ok(out)
    }
}

/// Validated, resolved compositor identity & command line — the subset of
/// `CompGlobals` filled for the bare-executable start case.
#[derive(Debug, Clone)]
pub struct CompGlobals {
    /// Full compositor command line.
    pub cmdline: Vec<String>,
    /// Internal id (basename of arg0).
    pub id: String,
    /// systemd-escaped id (for unit `%i`).
    pub id_unit_string: String,
    /// Binary basename.
    pub bin_name: String,
    /// Sanitized id for shell/plugin function names.
    pub bin_id: String,
    /// Final, de-duplicated desktop-name list.
    pub desktop_names: Vec<String>,
    /// Optional display name.
    pub name: Option<String>,
    /// Optional description.
    pub description: Option<String>,
}

/// Inputs to compositor resolution (the `start`-relevant subset for M0).
#[derive(Debug, Default)]
pub struct ResolveInput {
    /// Compositor command line (arg0 + args).
    pub wm_cmdline: Vec<String>,
    /// CLI `-D` desktop names (already split on `:`).
    pub desktop_names: Vec<String>,
    /// CLI `-e` exclusivity flag.
    pub desktop_names_exclusive: bool,
    /// CLI `-N` name.
    pub name: Option<String>,
    /// CLI `-C` description.
    pub description: Option<String>,
    /// `$XDG_CURRENT_DESKTOP` split on `:` (a desktop-name source when not exclusive).
    pub xdg_current_desktop: Vec<String>,
}

impl CompGlobals {
    /// Resolve a **bare-executable** compositor command (the priority case, as
    /// SDDM passes a plain command). Desktop-entry args return
    /// [`Error::NotImplemented`].
    pub fn resolve(input: &ResolveInput) -> Result<CompGlobals> {
        let arg0 = input
            .wm_cmdline
            .first()
            .ok_or_else(|| Error::InvalidArg("no compositor command given".into()))?;

        let main = MainArg::parse(arg0)?;
        if main.entry_id.is_some() {
            return Err(Error::todo("M3", "desktop-entry compositor resolution"));
        }

        let id = file_basename(arg0);
        if !is_wm_id(&id) {
            return Err(Error::Resolve(format!(
                "\"{id}\" is not a valid compositor id"
            )));
        }
        let id_unit_string = simple_systemd_escape(&id, false);
        let bin_name = id.clone();
        let bin_id = sanitize_bin_id(&bin_name);

        let mut desktop_names: Vec<String> = Vec::new();
        if input.desktop_names_exclusive {
            desktop_names.extend(input.desktop_names.iter().cloned());
        } else {
            desktop_names.extend(input.xdg_current_desktop.iter().cloned());
            desktop_names.extend(input.desktop_names.iter().cloned());
            if desktop_names.is_empty() {
                desktop_names.push(bin_name.clone());
            }
        }
        dedup_preserving(&mut desktop_names);

        Ok(CompGlobals {
            cmdline: input.wm_cmdline.clone(),
            id,
            id_unit_string,
            bin_name,
            bin_id,
            desktop_names,
            name: input.name.clone(),
            description: input.description.clone(),
        })
    }
}

fn is_action_id(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// `^[A-Za-z0-9_][A-Za-z0-9_.-]*\.desktop$` (uwsm `Val.entry_id`).
fn is_entry_id(s: &str) -> bool {
    if !s.ends_with(".desktop") {
        return false;
    }
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphanumeric() || c == '_' => {}
        _ => return false,
    }
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

/// `^[A-Za-z0-9_:.-]+$` (uwsm `Val.wm_id`).
fn is_wm_id(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | ':' | '.' | '-'))
}

fn file_basename(p: &str) -> String {
    p.rsplit('/').next().unwrap_or(p).to_string()
}

fn normalize_user_path(p: &str) -> PathBuf {
    let expanded = match p.strip_prefix("~/") {
        Some(rest) => match std::env::var("HOME") {
            Ok(home) => format!("{home}/{rest}"),
            Err(_) => p.to_string(),
        },
        None => p.to_string(),
    };
    PathBuf::from(expanded)
}

/// Port of `re.sub("(^[^a-zA-Z]|[^a-zA-Z0-9_])+", "_", name).lower()`: collapse
/// runs of non-word chars (and a leading non-letter) into single underscores.
fn sanitize_bin_id(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if (i == 0 && !c.is_ascii_alphabetic()) || !is_word(c) {
            if i == 0 && !c.is_ascii_alphabetic() {
                i += 1; // consume the leading non-letter (may be a word char)
            }
            while i < chars.len() && !is_word(chars[i]) {
                i += 1;
            }
            out.push('_');
        } else {
            out.push(c);
            i += 1;
        }
    }
    out.to_lowercase()
}

fn dedup_preserving(v: &mut Vec<String>) {
    let mut seen = HashSet::new();
    v.retain(|x| seen.insert(x.clone()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_executable() {
        let a = MainArg::parse("sway").unwrap();
        assert_eq!(a.executable.as_deref(), Some("sway"));
        assert!(a.entry_id.is_none());
        assert!(a.path.is_none());
    }

    #[test]
    fn classify_executable_path() {
        let a = MainArg::parse("/usr/bin/sway").unwrap();
        assert_eq!(a.executable.as_deref(), Some("/usr/bin/sway"));
        assert_eq!(a.path, Some(PathBuf::from("/usr/bin/sway")));
    }

    #[test]
    fn classify_entry_and_action() {
        let a = MainArg::parse("firefox.desktop").unwrap();
        assert_eq!(a.entry_id.as_deref(), Some("firefox.desktop"));
        assert!(a.entry_action.is_none());

        let b = MainArg::parse("firefox.desktop:new-window").unwrap();
        assert_eq!(b.entry_id.as_deref(), Some("firefox.desktop"));
        assert_eq!(b.entry_action.as_deref(), Some("new-window"));

        let c = MainArg::parse("firefox.desktop:").unwrap();
        assert_eq!(c.entry_action, None);
    }

    #[test]
    fn invalid_action_rejected() {
        assert!(MainArg::parse("x.desktop:bad action").is_err());
    }

    #[test]
    fn resolve_bare_exec() {
        let input = ResolveInput {
            wm_cmdline: vec!["sway".into(), "--arg".into()],
            xdg_current_desktop: vec!["Hyprland".into()],
            ..Default::default()
        };
        let cg = CompGlobals::resolve(&input).unwrap();
        assert_eq!(cg.id, "sway");
        assert_eq!(cg.id_unit_string, "sway");
        assert_eq!(cg.bin_id, "sway");
        assert_eq!(cg.cmdline, vec!["sway", "--arg"]);
        assert_eq!(cg.desktop_names, vec!["Hyprland"]);
    }

    #[test]
    fn resolve_desktop_names_fallback_to_binary() {
        let input = ResolveInput {
            wm_cmdline: vec!["sway".into()],
            ..Default::default()
        };
        let cg = CompGlobals::resolve(&input).unwrap();
        assert_eq!(cg.desktop_names, vec!["sway"]);
    }

    #[test]
    fn resolve_path_uses_basename_id() {
        let input = ResolveInput {
            wm_cmdline: vec!["/usr/local/bin/sway".into()],
            ..Default::default()
        };
        let cg = CompGlobals::resolve(&input).unwrap();
        assert_eq!(cg.id, "sway");
        assert_eq!(cg.cmdline, vec!["/usr/local/bin/sway"]);
    }

    #[test]
    fn resolve_entry_is_not_implemented() {
        let input = ResolveInput {
            wm_cmdline: vec!["sway.desktop".into()],
            ..Default::default()
        };
        assert!(matches!(
            CompGlobals::resolve(&input),
            Err(Error::NotImplemented { .. })
        ));
    }

    #[test]
    fn bin_id_sanitization() {
        assert_eq!(sanitize_bin_id("sway"), "sway");
        assert_eq!(sanitize_bin_id("my-comp"), "my_comp");
        assert_eq!(sanitize_bin_id("0sway"), "_sway");
        assert_eq!(sanitize_bin_id("Hyprland"), "hyprland");
        assert_eq!(sanitize_bin_id("a.b"), "a_b");
    }

    #[test]
    fn exclusive_desktop_names() {
        let input = ResolveInput {
            wm_cmdline: vec!["sway".into()],
            desktop_names: vec!["Custom".into()],
            desktop_names_exclusive: true,
            xdg_current_desktop: vec!["Ignored".into()],
            ..Default::default()
        };
        let cg = CompGlobals::resolve(&input).unwrap();
        assert_eq!(cg.desktop_names, vec!["Custom"]);
    }
}
