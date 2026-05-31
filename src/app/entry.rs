//! Minimal desktop-entry parser + validity checks. Ports the subset of pyxdg
//! plus `check_entry_basic` / `check_entry_showin` (`main.py:424`/`:501`) that
//! wsmr needs. See `REFERENCE.md` §13.5.

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
        if let Some(loc) = locale() {
            for cand in locale_variants(&loc) {
                if let Some(v) = g.get(&format!("{key}[{cand}]")) {
                    return Some(expand_str(v));
                }
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

    /// Basic validity: not hidden, `TryExec` resolves, action exists, and the
    /// `Exec` command is on `$PATH`.
    pub fn check_basic(&self, action: Option<&str>) -> Result<()> {
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
        if let Some(a) = action
            && !self.actions().iter().any(|x| x == a)
        {
            return Err(Error::InvalidArg(format!(
                "{} has no action {a}",
                self.filename
            )));
        }
        let exec = self.exec(action)?;
        let cmd = exec
            .first()
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

fn locale() -> Option<String> {
    for v in ["LC_MESSAGES", "LANG", "LC_ALL"] {
        if let Ok(s) = std::env::var(v)
            && !s.is_empty()
            && s != "C"
            && s != "POSIX"
        {
            return Some(s);
        }
    }
    None
}

/// `de_DE.UTF-8@mod` -> ["de_DE@mod","de_DE","de@mod","de"] (best-effort).
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
        let e =
            DesktopEntry::parse("/x/h.desktop", "[Desktop Entry]\nExec=sh\nHidden=true\n").unwrap();
        assert!(e.check_basic(None).is_err());
    }

    #[test]
    fn missing_exec_binary_fails_basic() {
        let e = DesktopEntry::parse(
            "/x/m.desktop",
            "[Desktop Entry]\nExec=definitely-not-a-real-binary-xyz\n",
        )
        .unwrap();
        assert!(e.check_basic(None).is_err());
        // a real binary passes
        let e2 = DesktopEntry::parse("/x/ok.desktop", "[Desktop Entry]\nExec=sh\n").unwrap();
        assert!(e2.check_basic(None).is_ok());
    }

    #[test]
    fn showin_filtering() {
        let e = entry(); // OnlyShowIn=GNOME;stub
        // not in desktop → discarded
        unsafe {
            std::env::set_var("XDG_CURRENT_DESKTOP", "KDE");
        }
        assert!(e.check_showin().is_err());
        unsafe {
            std::env::set_var("XDG_CURRENT_DESKTOP", "stub:other");
        }
        assert!(e.check_showin().is_ok());
        unsafe {
            std::env::remove_var("XDG_CURRENT_DESKTOP");
        }
    }
}
