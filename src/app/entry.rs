//! Minimal desktop-entry parser and validity checks for the subset wsmr uses.

use crate::app::field::{expand_str, tokenize_exec};
use crate::error::{Error, Result};
use crate::util;
use std::collections::BTreeMap;

/// A parsed desktop entry: groups of `key -> value`.
pub struct DesktopEntry {
    /// Entry file path (used for `%k` / `SourcePath`).
    pub filename: String,
    groups: BTreeMap<String, BTreeMap<String, String>>,
}

impl DesktopEntry {
    /// Parse entry `content` (file at `filename`).
    pub fn parse(filename: &str, content: &str) -> Result<DesktopEntry> {
        let mut groups: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        let mut current: Option<String> = None;
        for raw in content.lines() {
            let line = raw.trim_end_matches('\r');
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some(name) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                current = Some(name.to_string());
                groups.entry(name.to_string()).or_default();
            } else if let Some((k, v)) = line.split_once('=')
                && let Some(g) = &current
            {
                groups
                    .get_mut(g)
                    .unwrap()
                    .insert(k.trim().to_string(), v.to_string());
            }
        }
        if !groups.contains_key("Desktop Entry") {
            return Err(Error::InvalidArg(format!(
                "{filename}: missing [Desktop Entry] group"
            )));
        }
        Ok(DesktopEntry {
            filename: filename.to_string(),
            groups,
        })
    }

    fn group(&self, action: Option<&str>) -> Option<&BTreeMap<String, String>> {
        match action {
            None => self.groups.get("Desktop Entry"),
            Some(a) => self.groups.get(&format!("Desktop Action {a}")),
        }
    }

    /// Raw value of `key` in the given group (or the main group).
    pub fn get(&self, key: &str, action: Option<&str>) -> Option<&str> {
        self.group(action)?.get(key).map(String::as_str)
    }

    /// Locale-resolved, escape-expanded value of `key` (falls back to unlocalized).
    pub fn get_localized(&self, key: &str, action: Option<&str>) -> Option<String> {
        let g = self.group(action)?;
        for cand in locale_variant_chain() {
            if let Some(v) = g.get(&format!("{key}[{cand}]")) {
                return Some(expand_str(v));
            }
        }
        g.get(key).map(|v| expand_str(v))
    }

    /// Tokenized, expanded `Exec` for the group/action.
    pub fn exec(&self, action: Option<&str>) -> Result<Vec<String>> {
        let raw = self
            .get("Exec", action)
            .ok_or_else(|| Error::InvalidArg(format!("{}: no Exec", self.filename)))?;
        tokenize_exec(&expand_str(raw))
    }

    /// Whether the entry requests a terminal.
    pub fn terminal(&self) -> bool {
        self.get("Terminal", None) == Some("true")
    }

    /// `Path=` working directory, if any.
    pub fn path(&self) -> Option<&str> {
        self.get("Path", None).filter(|s| !s.is_empty())
    }

    /// Action ids from `Actions=`.
    pub fn actions(&self) -> Vec<String> {
        split_list(self.get("Actions", None))
    }

    /// `Categories=` entries.
    pub fn categories(&self) -> Vec<String> {
        split_list(self.get("Categories", None))
    }

    /// Basic validity: `Type=Application` with a non-empty `Name`, not
    /// hidden, `TryExec` resolves, the action (if any) has its own group with
    /// a `Name` and `Exec`, and the effective `Exec` command is on `$PATH`.
    /// This checks the fields required for launching without attempting full
    /// desktop-entry specification validation.
    pub fn check_basic(&self, action: Option<&str>) -> Result<()> {
        if self.get("Type", None) != Some("Application") {
            return Err(Error::InvalidArg(format!(
                "{} is not Type=Application",
                self.filename
            )));
        }
        if self.get("Name", None).is_none_or(str::is_empty) {
            return Err(Error::InvalidArg(format!("{} has no Name", self.filename)));
        }
        if self.get("Hidden", None) == Some("true") {
            return Err(Error::InvalidArg(format!("{} is hidden", self.filename)));
        }
        if let Some(tx) = self.get("TryExec", None)
            && !tx.is_empty()
            && util::which(tx).is_none()
        {
            return Err(Error::InvalidArg(format!(
                "{} discarded by TryExec ({tx})",
                self.filename
            )));
        }
        if let Some(a) = action {
            if !self.actions().iter().any(|x| x == a) {
                return Err(Error::InvalidArg(format!(
                    "{} has no action {a}",
                    self.filename
                )));
            }
            let Some(group) = self.group(Some(a)) else {
                return Err(Error::InvalidArg(format!(
                    "{} has no action group for {a}",
                    self.filename
                )));
            };
            if group.get("Name").is_none_or(|v| v.is_empty()) {
                return Err(Error::InvalidArg(format!(
                    "{} action {a} does not have Name",
                    self.filename
                )));
            }
            if group.get("Exec").is_none_or(|v| v.is_empty()) {
                return Err(Error::InvalidArg(format!(
                    "{} action {a} does not have Exec",
                    self.filename
                )));
            }
        }
        let exec = self.exec(action)?;
        let cmd = exec
            .first()
            .filter(|c| !c.is_empty())
            .ok_or_else(|| Error::InvalidArg(format!("{}: empty Exec", self.filename)))?;
        if util::which(cmd).is_none() {
            return Err(Error::InvalidArg(format!(
                "{} points to missing executable {cmd}",
                self.filename
            )));
        }
        Ok(())
    }

    /// `OnlyShowIn`/`NotShowIn` vs `$XDG_CURRENT_DESKTOP`.
    pub fn check_showin(&self) -> Result<()> {
        let xcd: Vec<String> =
            split_colon(&std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default());
        let osi = split_list(self.get("OnlyShowIn", None));
        let nsi = split_list(self.get("NotShowIn", None));
        if !osi.is_empty() && !osi.iter().any(|d| xcd.contains(d)) {
            return Err(Error::InvalidArg(format!(
                "{} discarded by OnlyShowIn",
                self.filename
            )));
        }
        if !nsi.is_empty() && nsi.iter().any(|d| xcd.contains(d)) {
            return Err(Error::InvalidArg(format!(
                "{} discarded by NotShowIn",
                self.filename
            )));
        }
        Ok(())
    }
}

fn split_list(v: Option<&str>) -> Vec<String> {
    v.unwrap_or("")
        .split(';')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn split_colon(s: &str) -> Vec<String> {
    s.split(':')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Resolve locale candidates in `LANGUAGE`, `LC_ALL`, `LC_MESSAGES`, `LANG`
/// order. The first non-empty variable wins; `LANGUAGE` may contain a
/// colon-separated preference list.
fn locale_candidates() -> Vec<String> {
    for var in ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(s) = std::env::var(var)
            && !s.is_empty()
        {
            return s
                .split(':')
                .filter(|v| !v.is_empty())
                .map(String::from)
                .collect();
        }
    }
    Vec::new()
}

/// Build the deduplicated locale-variant search order.
fn locale_variant_chain() -> Vec<String> {
    let mut out = Vec::new();
    for lang in locale_candidates() {
        for variant in locale_variants(&lang) {
            if !out.contains(&variant) {
                out.push(variant);
            }
        }
    }
    out
}

/// Expand `de_DE.UTF-8@mod` to
/// `["de_DE@mod", "de_DE", "de@mod", "de"]`.
fn locale_variants(loc: &str) -> Vec<String> {
    let no_codeset = loc.split('.').next().unwrap_or(loc); // strip .UTF-8
    let (base, modifier) = match no_codeset.split_once('@') {
        Some((b, m)) => (b, Some(m)),
        None => (no_codeset, None),
    };
    let lang = base.split('_').next().unwrap_or(base);
    let mut out = Vec::new();
    if let Some(m) = modifier {
        out.push(format!("{base}@{m}"));
    }
    out.push(base.to_string());
    if lang != base {
        if let Some(m) = modifier {
            out.push(format!("{lang}@{m}"));
        }
        out.push(lang.to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
[Desktop Entry]
Type=Application
Name=Web Browser
Name[de]=Webbrowser
GenericName=Browser
Exec=firefox %u
Icon=firefox
Terminal=false
Actions=new-window;
OnlyShowIn=GNOME;stub;

[Desktop Action new-window]
Name=New Window
Exec=firefox --new-window
";

    fn entry() -> DesktopEntry {
        DesktopEntry::parse("/x/firefox.desktop", SAMPLE).unwrap()
    }

    #[test]
    fn parses_keys_and_actions() {
        let e = entry();
        assert_eq!(e.get("Name", None), Some("Web Browser"));
        assert_eq!(e.get("Icon", None), Some("firefox"));
        assert_eq!(e.exec(None).unwrap(), vec!["firefox", "%u"]);
        assert_eq!(e.actions(), vec!["new-window"]);
        assert_eq!(
            e.exec(Some("new-window")).unwrap(),
            vec!["firefox", "--new-window"]
        );
        assert!(!e.terminal());
    }

    #[test]
    fn missing_group_errors() {
        assert!(DesktopEntry::parse("/x/b.desktop", "Name=x\n").is_err());
    }

    #[test]
    fn hidden_fails_basic() {
        let e = DesktopEntry::parse(
            "/x/h.desktop",
            "[Desktop Entry]\nType=Application\nName=H\nExec=sh\nHidden=true\n",
        )
        .unwrap();
        assert!(e.check_basic(None).is_err());
    }

    #[test]
    fn missing_type_or_name_fails_basic() {
        let no_type =
            DesktopEntry::parse("/x/nt.desktop", "[Desktop Entry]\nName=X\nExec=sh\n").unwrap();
        assert!(no_type.check_basic(None).is_err());
        let no_name = DesktopEntry::parse(
            "/x/nn.desktop",
            "[Desktop Entry]\nType=Application\nExec=sh\n",
        )
        .unwrap();
        assert!(no_name.check_basic(None).is_err());
        let empty_name = DesktopEntry::parse(
            "/x/en.desktop",
            "[Desktop Entry]\nType=Application\nName=\nExec=sh\n",
        )
        .unwrap();
        assert!(empty_name.check_basic(None).is_err());
    }

    #[test]
    fn missing_exec_binary_fails_basic() {
        let e = DesktopEntry::parse(
            "/x/m.desktop",
            "[Desktop Entry]\nType=Application\nName=M\nExec=definitely-not-a-real-binary-xyz\n",
        )
        .unwrap();
        assert!(e.check_basic(None).is_err());
        // a real binary passes
        let e2 = DesktopEntry::parse(
            "/x/ok.desktop",
            "[Desktop Entry]\nType=Application\nName=OK\nExec=sh\n",
        )
        .unwrap();
        assert!(e2.check_basic(None).is_ok());
    }

    #[test]
    fn showin_filtering() {
        use crate::testutil::with_env;
        let e = entry(); // OnlyShowIn=GNOME;stub
        with_env(&[("XDG_CURRENT_DESKTOP", Some("KDE"))], || {
            assert!(e.check_showin().is_err()); // not in OnlyShowIn
        });
        with_env(&[("XDG_CURRENT_DESKTOP", Some("stub:other"))], || {
            assert!(e.check_showin().is_ok());
        });
    }

    #[test]
    fn notshowin_filtering() {
        let e = DesktopEntry::parse(
            "/x/n.desktop",
            "[Desktop Entry]\nExec=sh\nNotShowIn=KDE;stub;\n",
        )
        .unwrap();
        use crate::testutil::with_env;
        with_env(&[("XDG_CURRENT_DESKTOP", Some("stub"))], || {
            assert!(e.check_showin().is_err()); // present in NotShowIn
        });
        with_env(&[("XDG_CURRENT_DESKTOP", Some("GNOME"))], || {
            assert!(e.check_showin().is_ok());
        });
    }

    #[test]
    fn terminal_path_categories_accessors() {
        let e = DesktopEntry::parse(
            "/x/t.desktop",
            "[Desktop Entry]\nExec=sh\nTerminal=true\nPath=/tmp\nCategories=Utility;TerminalEmulator;\n",
        )
        .unwrap();
        assert!(e.terminal());
        assert_eq!(e.path(), Some("/tmp"));
        assert_eq!(e.categories(), vec!["Utility", "TerminalEmulator"]);
        // empty Path → None
        let e2 = DesktopEntry::parse("/x/p.desktop", "[Desktop Entry]\nExec=sh\nPath=\n").unwrap();
        assert_eq!(e2.path(), None);
    }

    #[test]
    fn tryexec_discards_when_missing() {
        let e = DesktopEntry::parse(
            "/x/tx.desktop",
            "[Desktop Entry]\nType=Application\nName=TX\nExec=sh\nTryExec=definitely-not-real-bin-xyz\n",
        )
        .unwrap();
        assert!(e.check_basic(None).is_err());
        // TryExec that resolves passes
        let ok = DesktopEntry::parse(
            "/x/tx2.desktop",
            "[Desktop Entry]\nType=Application\nName=TX2\nExec=sh\nTryExec=/bin/sh\n",
        )
        .unwrap();
        assert!(ok.check_basic(None).is_ok());
    }

    #[test]
    fn unknown_action_rejected() {
        // Exec=sh so the executable check passes; isolates the action check.
        let e = DesktopEntry::parse(
            "/x/a.desktop",
            "[Desktop Entry]\nType=Application\nName=A\nExec=sh\nActions=go;\n[Desktop Action go]\nName=Go\nExec=sh\n",
        )
        .unwrap();
        assert!(e.check_basic(Some("no-such-action")).is_err());
        assert!(e.check_basic(Some("go")).is_ok());
    }

    #[test]
    fn action_without_group_name_or_exec_rejected() {
        // action listed in Actions= but its own group is missing entirely
        let no_group = DesktopEntry::parse(
            "/x/ng.desktop",
            "[Desktop Entry]\nType=Application\nName=NG\nExec=sh\nActions=go;\n",
        )
        .unwrap();
        assert!(no_group.check_basic(Some("go")).is_err());

        // action group present but missing Name
        let no_name = DesktopEntry::parse(
            "/x/nan.desktop",
            "[Desktop Entry]\nType=Application\nName=N\nExec=sh\nActions=go;\n[Desktop Action go]\nExec=sh\n",
        )
        .unwrap();
        assert!(no_name.check_basic(Some("go")).is_err());

        // action group present but missing Exec
        let no_exec = DesktopEntry::parse(
            "/x/nae.desktop",
            "[Desktop Entry]\nType=Application\nName=N\nExec=sh\nActions=go;\n[Desktop Action go]\nName=Go\n",
        )
        .unwrap();
        assert!(no_exec.check_basic(Some("go")).is_err());
    }

    #[test]
    fn localized_get_picks_locale_variant() {
        use crate::testutil::with_env;
        // Name[de] present in SAMPLE
        let e = entry();
        with_env(
            &[
                ("LANGUAGE", None),
                ("LC_ALL", None),
                ("LC_MESSAGES", None),
                ("LANG", Some("de_DE.UTF-8")),
            ],
            || assert_eq!(e.get_localized("Name", None).as_deref(), Some("Webbrowser")),
        );
        // C locale → unlocalized
        with_env(
            &[
                ("LANGUAGE", None),
                ("LC_ALL", None),
                ("LC_MESSAGES", None),
                ("LANG", Some("C")),
            ],
            || {
                assert_eq!(
                    e.get_localized("Name", None).as_deref(),
                    Some("Web Browser")
                )
            },
        );
    }

    /// Precedence must be `LANGUAGE`, then `LC_ALL`, then `LC_MESSAGES`, then
    /// `LANG`; the first set variable wins outright.
    #[test]
    fn locale_precedence_language_then_lc_all_then_lc_messages_then_lang() {
        use crate::testutil::with_env;
        let e = entry();
        // LC_ALL beats LC_MESSAGES and LANG
        with_env(
            &[
                ("LANGUAGE", None),
                ("LC_ALL", Some("de_DE")),
                ("LC_MESSAGES", Some("fr_FR")),
                ("LANG", Some("fr_FR")),
            ],
            || assert_eq!(e.get_localized("Name", None).as_deref(), Some("Webbrowser")),
        );
        // LC_MESSAGES beats LANG when LC_ALL is unset
        with_env(
            &[
                ("LANGUAGE", None),
                ("LC_ALL", None),
                ("LC_MESSAGES", Some("de_DE")),
                ("LANG", Some("fr_FR")),
            ],
            || assert_eq!(e.get_localized("Name", None).as_deref(), Some("Webbrowser")),
        );
        // LANGUAGE beats everything, including a colon-separated list
        with_env(
            &[
                ("LANGUAGE", Some("de:fr")),
                ("LC_ALL", Some("fr_FR")),
                ("LC_MESSAGES", None),
                ("LANG", None),
            ],
            || assert_eq!(e.get_localized("Name", None).as_deref(), Some("Webbrowser")),
        );
    }

    #[test]
    fn locale_variants_expands() {
        assert_eq!(
            locale_variants("de_DE@euro"),
            vec!["de_DE@euro", "de_DE", "de@euro", "de"]
        );
        // codeset is stripped (best-effort): everything after the first '.'
        assert_eq!(locale_variants("de_DE.UTF-8"), vec!["de_DE", "de"]);
        assert_eq!(locale_variants("fr"), vec!["fr"]);
    }

    #[test]
    fn locale_candidates_splits_language_on_colon() {
        use crate::testutil::with_env;
        with_env(
            &[("LANGUAGE", Some("de_DE:fr_FR")), ("LC_ALL", None)],
            || {
                assert_eq!(locale_candidates(), vec!["de_DE", "fr_FR"]);
            },
        );
    }
}
