//! Terminal-emulator resolution + command assembly for `wsmr app -T` and
//! entries with `Terminal=true`. Ports `find_terminal_entry` (`main.py:3170`)
//! and the terminal-handling block of `app()` (`:3537`). See `REFERENCE.md`
//! §13.6.
//!
//! The not-terminals neg-cache (a perf optimization) is intentionally omitted —
//! we just scan.

use crate::app::entry::DesktopEntry;
use crate::app::field::expand_str;
use crate::app::find;
use crate::comp::MainArg;
use crate::error::{Error, Result};
use crate::util::xdg;
use std::collections::HashSet;

/// Per-launch terminal options (mapped to the entry's `TerminalArg*` keys).
#[derive(Default)]
pub struct TermOpts {
    /// `--app-id` value.
    pub app_id: Option<String>,
    /// `--title` value.
    pub title: Option<String>,
    /// Working directory.
    pub dir: Option<String>,
    /// Keep the terminal open after the command exits.
    pub hold: bool,
}

/// Resolve a terminal emulator and assemble `(cmdline, exec_arg)`: the terminal
/// command (with options) and the argument used to pass a command to it.
pub fn resolve_terminal(opts: &TermOpts) -> Result<(Vec<String>, Vec<String>)> {
    let (entry, action) = find_terminal_entry()?;
    build_terminal(&entry, action.as_deref(), opts)
}

/// Build the terminal cmdline + exec-arg from a resolved entry (pure; tested).
pub fn build_terminal(
    entry: &DesktopEntry,
    action: Option<&str>,
    opts: &TermOpts,
) -> Result<(Vec<String>, Vec<String>)> {
    let mut cmdline = entry.exec(action)?;

    let exec_arg_raw = first_key(
        entry,
        &[
            "TerminalArgExec",
            "X-TerminalArgExec",
            "ExecArg",
            "X-ExecArg",
        ],
    )
    .map(|s| expand_str(&s))
    .unwrap_or_else(|| "-e".to_string());
    let exec_arg = if exec_arg_raw.is_empty() {
        Vec::new()
    } else {
        vec![exec_arg_raw]
    };

    if let Some(v) = &opts.app_id {
        append_opt(
            &mut cmdline,
            entry,
            &["TerminalArgAppId", "X-TerminalArgAppId"],
            v,
        );
    }
    if let Some(v) = &opts.title {
        append_opt(
            &mut cmdline,
            entry,
            &["TerminalArgTitle", "X-TerminalArgTitle"],
            v,
        );
    }
    if let Some(v) = &opts.dir {
        append_opt(
            &mut cmdline,
            entry,
            &["TerminalArgDir", "X-TerminalArgDir"],
            v,
        );
    }
    if opts.hold
        && let Some(h) = first_key(entry, &["TerminalArgHold", "X-TerminalArgHold"])
    {
        cmdline.push(h);
    }
    Ok((cmdline, exec_arg))
}

fn first_key(entry: &DesktopEntry, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| entry.get(k, None))
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn append_opt(cmdline: &mut Vec<String>, entry: &DesktopEntry, keys: &[&str], value: &str) {
    let Some(arg) = first_key(entry, keys) else {
        return;
    };
    match arg.strip_suffix('=') {
        Some(prefix) => cmdline.push(format!("{prefix}={value}")),
        None => {
            cmdline.push(arg);
            cmdline.push(value.to_string());
        }
    }
}

/// Find the preferred terminal emulator: honor the `xdg-terminals.list` files,
/// else scan `applications` for a `TerminalEmulator`.
pub fn find_terminal_entry() -> Result<(DesktopEntry, Option<String>)> {
    let (explicit, excluded) = read_terminal_lists();

    for (id, action) in &explicit {
        if let Some(e) = find::find_entry("applications", id)?
            && is_terminal(&e)
            && e.check_basic(action.as_deref()).is_ok()
        {
            return Ok((e, action.clone()));
        }
    }

    let found = find::find_first("applications", |id, e| {
        !excluded.contains(id)
            && is_terminal(e)
            && e.check_basic(None).is_ok()
            && e.check_showin().is_ok()
    })?;

    found
        .map(|e| (e, None))
        .ok_or_else(|| Error::Resolve("could not find a terminal emulator".into()))
}

fn is_terminal(e: &DesktopEntry) -> bool {
    e.categories().iter().any(|c| c == "TerminalEmulator")
}

/// Read `<desktop>-xdg-terminals.list` + `xdg-terminals.list` from config dirs
/// and system `xdg-terminal-exec` data dirs. Returns the explicit preference
/// list and the set of excluded ids.
fn read_terminal_lists() -> (Vec<(String, Option<String>)>, HashSet<String>) {
    let mut explicit: Vec<(String, Option<String>)> = Vec::new();
    let mut excluded: HashSet<String> = HashSet::new();
    let mut protected: HashSet<String> = HashSet::new();

    let mut files: Vec<String> = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .filter(|s| !s.is_empty())
        .map(|d| format!("{d}-xdg-terminals.list"))
        .collect();
    files.push("xdg-terminals.list".to_string());

    let mut dirs = xdg::config_paths();
    for d in xdg::data_dirs() {
        dirs.push(d.join("xdg-terminal-exec"));
    }

    for dir in dirs {
        for f in &files {
            let Ok(content) = std::fs::read_to_string(dir.join(f)) else {
                continue;
            };
            parse_terminal_list(&content, &mut explicit, &mut excluded, &mut protected);
        }
    }
    (explicit, excluded)
}

/// Apply one `xdg-terminals.list` file's lines to the running preference state.
/// `+id` protects/un-excludes, `-id` excludes, a bare `id` appends to the
/// preference list; `#` lines and blanks are ignored. Pure (no I/O).
fn parse_terminal_list(
    content: &str,
    explicit: &mut Vec<(String, Option<String>)>,
    excluded: &mut HashSet<String>,
    protected: &mut HashSet<String>,
) {
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (fb, rest) = match line.strip_prefix('-') {
            Some(r) => (-1i8, r),
            None => match line.strip_prefix('+') {
                Some(r) => (1, r),
                None => (0, line),
            },
        };
        let (id, action) = parse_entry_ref(rest);
        let Some(id) = id else { continue };
        match fb {
            0 if !explicit.iter().any(|(i, a)| i == &id && a == &action) => {
                explicit.push((id, action));
            }
            -1 if action.is_none() && !protected.contains(&id) => {
                excluded.insert(id);
            }
            1 if action.is_none() => {
                protected.insert(id.clone());
                excluded.remove(&id);
            }
            _ => {}
        }
    }
}

fn parse_entry_ref(s: &str) -> (Option<String>, Option<String>) {
    match MainArg::parse(s) {
        Ok(m) if m.entry_id.is_some() && m.path.is_none() => (m.entry_id, m.entry_action),
        _ => (None, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term_entry(extra: &str) -> DesktopEntry {
        let content = format!(
            "[Desktop Entry]\nType=Application\nExec=foot\nCategories=TerminalEmulator;\n{extra}"
        );
        DesktopEntry::parse("/x/foot.desktop", &content).unwrap()
    }

    #[test]
    fn default_exec_arg_is_dash_e() {
        let e = term_entry("");
        let (cmd, exec_arg) = build_terminal(&e, None, &TermOpts::default()).unwrap();
        assert_eq!(cmd, vec!["foot"]);
        assert_eq!(exec_arg, vec!["-e"]);
    }

    #[test]
    fn options_appended_per_terminalarg_keys() {
        let e = term_entry(
            "TerminalArgExec=-e\nTerminalArgAppId=--app-id\nTerminalArgTitle=--title=\n",
        );
        let opts = TermOpts {
            app_id: Some("myapp".into()),
            title: Some("My Title".into()),
            ..Default::default()
        };
        let (cmd, exec_arg) = build_terminal(&e, None, &opts).unwrap();
        assert_eq!(cmd, vec!["foot", "--app-id", "myapp", "--title=My Title"]);
        assert_eq!(exec_arg, vec!["-e"]);
    }

    #[test]
    fn hold_flag() {
        let e = term_entry("TerminalArgHold=--hold\n");
        let opts = TermOpts {
            hold: true,
            ..Default::default()
        };
        let (cmd, _) = build_terminal(&e, None, &opts).unwrap();
        assert!(cmd.contains(&"--hold".to_string()));
    }

    #[test]
    fn custom_exec_arg() {
        let e = term_entry("TerminalArgExec=--command\n");
        let (_, exec_arg) = build_terminal(&e, None, &TermOpts::default()).unwrap();
        assert_eq!(exec_arg, vec!["--command"]);
        // empty key values are treated as absent → default "-e"
        let e = term_entry("TerminalArgExec=\nX-ExecArg=\n");
        let (_, exec_arg) = build_terminal(&e, None, &TermOpts::default()).unwrap();
        assert_eq!(exec_arg, vec!["-e"]);
    }

    #[test]
    fn dir_option_and_no_hold_key() {
        let e = term_entry("TerminalArgDir=--working-directory=\n");
        let opts = TermOpts {
            dir: Some("/tmp".into()),
            hold: true, // no TerminalArgHold key → nothing appended
            ..Default::default()
        };
        let (cmd, _) = build_terminal(&e, None, &opts).unwrap();
        assert!(cmd.contains(&"--working-directory=/tmp".to_string()));
    }

    #[test]
    fn is_terminal_checks_category() {
        assert!(is_terminal(&term_entry("")));
        let not = DesktopEntry::parse(
            "/x/x.desktop",
            "[Desktop Entry]\nExec=x\nCategories=Utility;\n",
        )
        .unwrap();
        assert!(!is_terminal(&not));
    }

    #[test]
    fn parse_list_fallback_prefixes() {
        let mut explicit = Vec::new();
        let mut excluded = HashSet::new();
        let mut protected = HashSet::new();
        parse_terminal_list(
            "# comment\n\nfoo.desktop\n-bar.desktop\nfoo.desktop\n",
            &mut explicit,
            &mut excluded,
            &mut protected,
        );
        // bare id appended once (dup ignored); '-' excludes
        assert_eq!(explicit, vec![("foo.desktop".to_string(), None)]);
        assert!(excluded.contains("bar.desktop"));

        // '+' protects so a later '-' cannot exclude it
        let mut explicit = Vec::new();
        let mut excluded = HashSet::new();
        let mut protected = HashSet::new();
        parse_terminal_list(
            "+keep.desktop\n-keep.desktop\n",
            &mut explicit,
            &mut excluded,
            &mut protected,
        );
        assert!(protected.contains("keep.desktop"));
        assert!(!excluded.contains("keep.desktop"));
    }

    #[test]
    fn find_terminal_entry_from_list_and_scan() {
        use crate::testutil::with_env;
        let root = std::env::temp_dir().join(format!("wsmr-term-{}", std::process::id()));
        let apps = root.join("applications");
        std::fs::create_dir_all(&apps).unwrap();
        // a terminal whose Exec is on PATH (`sh`) so check_basic passes
        std::fs::write(
            apps.join("myterm.desktop"),
            "[Desktop Entry]\nType=Application\nExec=sh\nCategories=TerminalEmulator;\n",
        )
        .unwrap();
        // explicit preference file
        let cfg = root.join("config");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(cfg.join("xdg-terminals.list"), "myterm.desktop\n").unwrap();

        with_env(
            &[
                ("XDG_DATA_HOME", Some(root.to_str().unwrap())),
                ("XDG_DATA_DIRS", Some("")),
                ("XDG_CONFIG_HOME", Some(cfg.to_str().unwrap())),
                ("XDG_CONFIG_DIRS", Some("")),
                ("XDG_CURRENT_DESKTOP", Some("stub")),
            ],
            || {
                let (e, action) = find_terminal_entry().unwrap();
                assert!(is_terminal(&e));
                assert!(action.is_none());
            },
        );

        // remove the list → falls back to category scan, still finds myterm
        std::fs::remove_file(cfg.join("xdg-terminals.list")).unwrap();
        with_env(
            &[
                ("XDG_DATA_HOME", Some(root.to_str().unwrap())),
                ("XDG_DATA_DIRS", Some("")),
                ("XDG_CONFIG_HOME", Some(cfg.to_str().unwrap())),
                ("XDG_CONFIG_DIRS", Some("")),
                ("XDG_CURRENT_DESKTOP", Some("stub")),
            ],
            || assert!(find_terminal_entry().is_ok()),
        );

        // no terminals anywhere → error
        std::fs::remove_file(apps.join("myterm.desktop")).unwrap();
        with_env(
            &[
                ("XDG_DATA_HOME", Some(root.to_str().unwrap())),
                ("XDG_DATA_DIRS", Some("")),
                ("XDG_CONFIG_HOME", Some(cfg.to_str().unwrap())),
                ("XDG_CONFIG_DIRS", Some("")),
                ("XDG_CURRENT_DESKTOP", Some("stub")),
            ],
            || assert!(find_terminal_entry().is_err()),
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
