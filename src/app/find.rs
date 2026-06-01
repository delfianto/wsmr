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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::with_env;

    /// Build `<tmp>/applications/...` with the given `(relpath, contents)` files
    /// and return the data-home root.
    fn data_home(files: &[(&str, &str)]) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "wsmr-find-{}-{:p}",
            std::process::id(),
            files as *const _
        ));
        let apps = root.join("applications");
        for (rel, content) in files {
            let p = apps.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, content).unwrap();
        }
        root
    }

    fn isolate<T>(root: &Path, f: impl FnOnce() -> T) -> T {
        with_env(
            &[
                ("XDG_DATA_HOME", Some(root.to_str().unwrap())),
                ("XDG_DATA_DIRS", Some("")), // no system dirs
            ],
            f,
        )
    }

    #[test]
    fn find_entry_top_level_and_nested() {
        let root = data_home(&[
            ("foo.desktop", "[Desktop Entry]\nExec=foo\n"),
            ("sub/bar.desktop", "[Desktop Entry]\nExec=bar\n"),
            ("note.txt", "ignored"),
        ]);
        isolate(&root, || {
            assert!(find_entry("applications", "foo.desktop").unwrap().is_some());
            // nested id has '/' replaced by '-'
            assert!(
                find_entry("applications", "sub-bar.desktop")
                    .unwrap()
                    .is_some()
            );
            // non-.desktop and unknown id miss
            assert!(find_entry("applications", "note.txt").unwrap().is_none());
            assert!(
                find_entry("applications", "nope.desktop")
                    .unwrap()
                    .is_none()
            );
            // missing subdir is fine
            assert!(
                find_entry("missing-subdir", "foo.desktop")
                    .unwrap()
                    .is_none()
            );
        });
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn find_first_matches_predicate() {
        let root = data_home(&[
            (
                "a.desktop",
                "[Desktop Entry]\nExec=a\nCategories=Utility;\n",
            ),
            (
                "term.desktop",
                "[Desktop Entry]\nExec=term\nCategories=TerminalEmulator;\n",
            ),
            ("broken.desktop", "no group here\n"), // fails parse → skipped
        ]);
        isolate(&root, || {
            let found = find_first("applications", |_id, e| {
                e.categories().iter().any(|c| c == "TerminalEmulator")
            })
            .unwrap();
            assert!(found.is_some());
            assert_eq!(found.unwrap().exec(None).unwrap(), vec!["term"]);
            // predicate never matches → None
            assert!(find_first("applications", |_, _| false).unwrap().is_none());
        });
        let _ = std::fs::remove_dir_all(&root);
    }
}
