//! Locate desktop entries by id across the XDG data hierarchy. Ports the
//! discovery in `find_entries` (`main.py:625`): walk `<data_dir>/<subpath>`,
//! id = path relative to the data dir with `/` → `-`, first match wins.

use crate::app::entry::DesktopEntry;
use crate::error::{Error, Result};
use crate::util::xdg;
use std::path::Path;

/// Find an entry by id (e.g. `firefox.desktop`) under `subpath`
/// (e.g. `applications`). Returns the first match in data-dir order.
pub fn find_entry(subpath: &str, id: &str) -> Result<Option<DesktopEntry>> {
    for base in xdg::data_paths() {
        let dir = base.join(subpath);
        if !dir.is_dir() {
            continue;
        }
        if let Some(e) = walk(&dir, &dir, id)? {
            return Ok(Some(e));
        }
    }
    Ok(None)
}

/// Walk entries under `subpath` and return the first whose `(id, entry)` matches
/// `pred`. Used for category scans (e.g. terminal emulators).
pub fn find_first<F>(subpath: &str, mut pred: F) -> Result<Option<DesktopEntry>>
where
    F: FnMut(&str, &DesktopEntry) -> bool,
{
    for base in xdg::data_paths() {
        let dir = base.join(subpath);
        if !dir.is_dir() {
            continue;
        }
        if let Some(e) = walk_pred(&dir, &dir, &mut pred)? {
            return Ok(Some(e));
        }
    }
    Ok(None)
}

fn walk_pred<F>(base: &Path, dir: &Path, pred: &mut F) -> Result<Option<DesktopEntry>>
where
    F: FnMut(&str, &DesktopEntry) -> bool,
{
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return Ok(None),
    };
    for entry in rd {
        let entry = entry.map_err(|e| Error::io(dir, e))?;
        let path = entry.path();
        if path.is_dir() {
            if let Some(e) = walk_pred(base, &path, pred)? {
                return Ok(Some(e));
            }
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("desktop") {
            continue;
        }
        let id = path
            .strip_prefix(base)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('/', "-");
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if let Ok(e) = DesktopEntry::parse(&path.to_string_lossy(), &content)
            && pred(&id, &e)
        {
            return Ok(Some(e));
        }
    }
    Ok(None)
}

fn walk(base: &Path, dir: &Path, id: &str) -> Result<Option<DesktopEntry>> {
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return Ok(None),
    };
    for entry in rd {
        let entry = entry.map_err(|e| Error::io(dir, e))?;
        let path = entry.path();
        if path.is_dir() {
            if let Some(e) = walk(base, &path, id)? {
                return Ok(Some(e));
            }
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("desktop") {
            continue;
        }
        let rel = path
            .strip_prefix(base)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('/', "-");
        if rel == id {
            let content = std::fs::read_to_string(&path).map_err(|e| Error::io(&path, e))?;
            return Ok(Some(DesktopEntry::parse(
                &path.to_string_lossy(),
                &content,
            )?));
        }
    }
    Ok(None)
}
