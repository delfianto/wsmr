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
