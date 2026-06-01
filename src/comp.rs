//! Compositor argument classification and resolution.
//!
//! Ports `MainArg` (`main.py:188`) and the **bare-executable** branch of
//! `fill_comp_globals` (`main.py:3965`/`:4292`). Desktop-entry compositor
//! resolution is deferred to M3. See `docs/uwsm-core-analysis.md` §3.2/§6.

use crate::app::entry::DesktopEntry;
use crate::app::find;
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
    /// Desktop names exactly as passed on the CLI via `-D` (recorded in the
    /// drop-in so a regenerated unit re-passes them verbatim).
    pub cli_desktop_names: Vec<String>,
    /// Whether `-e` (exclusive desktop names) was set on the CLI.
    pub cli_desktop_names_exclusive: bool,
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
        if let Some(entry_id) = main.entry_id.clone() {
            return resolve_entry(input, &main, &entry_id);
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
            cli_desktop_names: input.desktop_names.clone(),
            cli_desktop_names_exclusive: input.desktop_names_exclusive,
        })
    }
}

/// Resolve a **desktop-entry** compositor (`wsmr start <id>.desktop` or a path to
/// one): load the entry from `wayland-sessions` (or the given path), take its
/// `Exec` as the command line, and pull `DesktopNames`/`Name`/`Comment` as
/// defaults (CLI `-D`/`-N`/`-C` still win). Mirrors uwsm's entry branch of
/// `fill_comp_globals`.
fn resolve_entry(input: &ResolveInput, main: &MainArg, entry_id: &str) -> Result<CompGlobals> {
    let entry = match &main.path {
        Some(p) => {
            let content = std::fs::read_to_string(p).map_err(|e| Error::io(p, e))?;
            DesktopEntry::parse(&p.to_string_lossy(), &content)?
        }
        None => find::find_entry("wayland-sessions", entry_id)?
            .ok_or_else(|| Error::Resolve(format!("compositor entry not found: {entry_id}")))?,
    };
    let action = main.entry_action.as_deref();
    entry.check_basic(action)?;

    // command line = the entry's Exec, plus any args after the entry on the CLI
    let mut cmdline = entry.exec(action)?;
    cmdline.extend(input.wm_cmdline.iter().skip(1).cloned());

    // id = the entry's filename stem (units become wayland-wm@<id>)
    let id = entry_id.trim_end_matches(".desktop").to_string();
    if !is_wm_id(&id) {
        return Err(Error::Resolve(format!(
            "\"{id}\" is not a valid compositor id"
        )));
    }
    let id_unit_string = simple_systemd_escape(&id, false);
    let bin_name = file_basename(cmdline.first().map(String::as_str).unwrap_or(&id));
    let bin_id = sanitize_bin_id(&bin_name);

    // desktop names: same merge as the bare case, but the entry's `DesktopNames`
    // is an additional source (after XDG_CURRENT_DESKTOP + CLI `-D`).
    let entry_dn: Vec<String> = entry
        .get("DesktopNames", None)
        .map(|s| {
            s.split(';')
                .filter(|x| !x.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let mut desktop_names: Vec<String> = Vec::new();
    if input.desktop_names_exclusive {
        desktop_names.extend(input.desktop_names.iter().cloned());
    } else {
        desktop_names.extend(input.xdg_current_desktop.iter().cloned());
        desktop_names.extend(input.desktop_names.iter().cloned());
        desktop_names.extend(entry_dn);
        if desktop_names.is_empty() {
            desktop_names.push(bin_name.clone());
        }
    }
    dedup_preserving(&mut desktop_names);

    let name = input
        .name
        .clone()
        .or_else(|| entry.get_localized("Name", action));
    let description = input
        .description
        .clone()
        .or_else(|| entry.get_localized("Comment", action));

    Ok(CompGlobals {
        cmdline,
        id,
        id_unit_string,
        bin_name,
        bin_id,
        desktop_names,
        name,
        description,
        cli_desktop_names: input.desktop_names.clone(),
        cli_desktop_names_exclusive: input.desktop_names_exclusive,
    })
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
    fn resolve_entry_not_found_errors() {
        // a bare entry id that isn't in any wayland-sessions dir → Resolve error
        let input = ResolveInput {
            wm_cmdline: vec!["wsmr-no-such-compositor.desktop".into()],
            ..Default::default()
        };
        assert!(matches!(
            CompGlobals::resolve(&input),
            Err(Error::Resolve(_))
        ));
    }

    #[test]
    fn resolve_entry_by_path() {
        // a path to a wayland-sessions entry is loaded directly
        let dir = std::env::temp_dir().join(format!("wsmr-comp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("mycomp.desktop");
        std::fs::write(
            &path,
            "[Desktop Entry]\nType=Application\nName=My Comp\nComment=test wm\nExec=sh\nDesktopNames=mycomp;wlroots;\n",
        )
        .unwrap();
        let input = ResolveInput {
            wm_cmdline: vec![path.to_string_lossy().into_owned(), "--debug".into()],
            xdg_current_desktop: vec!["X-Generic".into()],
            ..Default::default()
        };
        let cg = CompGlobals::resolve(&input).unwrap();
        assert_eq!(cg.id, "mycomp");
        assert_eq!(cg.cmdline, vec!["sh", "--debug"]); // Exec + trailing CLI arg
        assert_eq!(cg.bin_name, "sh");
        assert_eq!(cg.name.as_deref(), Some("My Comp"));
        assert_eq!(cg.description.as_deref(), Some("test wm"));
        // XDG_CURRENT_DESKTOP, then the entry's DesktopNames
        assert_eq!(cg.desktop_names, vec!["X-Generic", "mycomp", "wlroots"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_entry_cli_name_overrides() {
        let dir = std::env::temp_dir().join(format!("wsmr-comp2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("c.desktop");
        std::fs::write(&path, "[Desktop Entry]\nName=Entry Name\nExec=sh\n").unwrap();
        let input = ResolveInput {
            wm_cmdline: vec![path.to_string_lossy().into_owned()],
            name: Some("CLI Name".into()),
            desktop_names: vec!["only".into()],
            desktop_names_exclusive: true,
            xdg_current_desktop: vec!["ignored".into()],
            ..Default::default()
        };
        let cg = CompGlobals::resolve(&input).unwrap();
        assert_eq!(cg.name.as_deref(), Some("CLI Name")); // CLI -N wins
        assert_eq!(cg.desktop_names, vec!["only"]); // exclusive → CLI only
        assert!(cg.cli_desktop_names_exclusive);
        let _ = std::fs::remove_dir_all(&dir);
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
    fn classify_entry_path_keeps_basename_id() {
        let a = MainArg::parse("/apps/firefox.desktop").unwrap();
        assert_eq!(a.entry_id.as_deref(), Some("firefox.desktop"));
        assert_eq!(a.path, Some(PathBuf::from("/apps/firefox.desktop")));
        // with an action
        let b = MainArg::parse("/apps/firefox.desktop:new-window").unwrap();
        assert_eq!(b.entry_id.as_deref(), Some("firefox.desktop"));
        assert_eq!(b.entry_action.as_deref(), Some("new-window"));
    }

    #[test]
    fn tilde_path_is_home_expanded() {
        use crate::testutil::with_env;
        with_env(&[("HOME", Some("/home/u"))], || {
            let a = MainArg::parse("~/x/foo.desktop").unwrap();
            assert_eq!(a.path, Some(PathBuf::from("/home/u/x/foo.desktop")));
        });
    }

    #[test]
    fn invalid_entry_id_rejected() {
        // leading '-' is not a valid entry-id start
        assert!(MainArg::parse("-bad.desktop").is_err());
    }

    #[test]
    fn resolve_empty_cmdline_errors() {
        assert!(CompGlobals::resolve(&ResolveInput::default()).is_err());
    }

    #[test]
    fn resolve_invalid_id_errors() {
        let input = ResolveInput {
            wm_cmdline: vec!["sway!bang".into()],
            ..Default::default()
        };
        assert!(matches!(
            CompGlobals::resolve(&input),
            Err(Error::Resolve(_))
        ));
    }

    #[test]
    fn is_entry_id_rules() {
        assert!(is_entry_id("foo.desktop"));
        assert!(is_entry_id("_x.desktop"));
        assert!(!is_entry_id("nope")); // no suffix
        assert!(!is_entry_id("-x.desktop")); // bad first char
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
