//! The shared session-state files under `$XDG_RUNTIME_DIR/wsmr/`
//! (`env_pre`, `env_cleanup.list`, `generation`) and the single OS-backed
//! lock serializing every read-modify-write against them.
//!
//! `prepare-env`, `finalize`, the readiness watcher, and `cleanup-env` each
//! run as **separate processes** and can race on these files — this module
//! is the only place that touches them, so every caller gets the same
//! locking and generation-scoping for free rather than having to remember it
//! at each call site. Ports the various read/write points scattered across
//! `main.py:2682`/`:2424`/`:5066`/`:2922`. See `REFERENCE.md` §3/§4/§6.
//!
//! **Locking.** [`lock`] uses `std::fs::File::lock` — an OS-level advisory
//! lock (`flock(2)` on Unix) — so it has clean crash semantics for free: a
//! process that dies while holding it has the lock released by the kernel
//! when its file descriptor is closed, with no stale-lockfile cleanup
//! required. There's only ever one lock, so there's no ordering to get wrong.
//!
//! **Generations.** Every session gets a fresh random id, minted by
//! [`begin_generation`] and recorded alongside `env_pre`. Cleanup-list
//! entries carry the generation id that requested them
//! ([`crate::env::files::CleanupEntry`]), so [`end_generation`] only ever
//! acts on entries belonging to the generation currently on record — a line
//! left over by a different session can't be picked up as this session's.
//!
//! **Residual gap:** a fresh generation always starts by resolving any
//! abandoned prior state first (see [`begin_generation`]), which closes the
//! common case — a crash that skipped `cleanup-env` entirely. What isn't
//! closed: if an old session's `cleanup-env` (its unit's `ExecStopPost`) is
//! *still running* at the exact moment a brand new session's `prepare-env`
//! starts — a narrow window, since `start` already refuses to begin while
//! any compositor unit is active/activating — the late `cleanup-env` has no
//! way to carry its own generation id forward across the process boundary
//! (unit templates are static; the id only exists at runtime), so it will
//! act on whatever generation is current by the time it acquires the lock.

use crate::env::files::{self, CleanupEntry};
use crate::error::{Error, Result};
use crate::session::runtime_path;
use crate::sysd::dbus::SessionBus;
use crate::util::fsutil;
use crate::varnames;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

/// Held for the span of a critical section; the OS releases the underlying
/// advisory lock when this drops.
struct Lock(#[allow(dead_code)] File);

fn lock() -> Result<Lock> {
    let path = runtime_path("state.lock")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| Error::io(&path, e))?;
    file.lock().map_err(|e| Error::io(&path, e))?;
    Ok(Lock(file))
}

fn generation_path() -> Result<PathBuf> {
    runtime_path("generation")
}
fn env_pre_path() -> Result<PathBuf> {
    runtime_path("env_pre")
}
fn cleanup_path() -> Result<PathBuf> {
    runtime_path("env_cleanup.list")
}

/// A fresh, unique-enough-in-practice generation id.
fn new_generation_id() -> String {
    random_hex_id()
}

/// 16-hex-char random id, sourced from `/dev/urandom` with a time-based
/// fallback when it's unavailable (e.g. a restricted test sandbox).
pub(crate) fn random_hex_id() -> String {
    let mut buf = [0u8; 8];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut buf);
    } else {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        buf.copy_from_slice(&n.to_le_bytes());
    }
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

fn read_generation(path: &PathBuf) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(Some(s.trim().to_string()).filter(|s| !s.is_empty())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::io(path, e)),
    }
}

/// Begin a fresh generation: resolve any abandoned prior generation's state
/// first (see module docs), then establish a new generation id and `env_pre`
/// snapshot with an empty cleanup list. Returns the new generation id.
///
/// Called once by `prepare-env` at the start of a session.
pub fn begin_generation(bus: &SessionBus, pre: &BTreeMap<String, String>) -> Result<String> {
    let _guard = lock()?;
    restore_and_clear_locked(bus)?;

    let id = new_generation_id();
    fsutil::atomic_write_path(&generation_path()?, &format!("{id}\n"))?;
    files::save_env(&env_pre_path()?, pre, files::Sep::Nul)?;
    files::write_cleanup_entries(&cleanup_path()?, &BTreeSet::new())?;
    Ok(id)
}

/// Merge `names` into the cleanup list, tagged with whatever generation is
/// currently on record. Entries from other generations already in the file
/// are left untouched. Errors if no generation is currently established
/// (nothing to safely attribute the names to — see module docs).
pub fn append_cleanup(names: impl IntoIterator<Item = String>) -> Result<()> {
    let _guard = lock()?;
    let gen_path = generation_path()?;
    let Some(generation) = read_generation(&gen_path)? else {
        return Err(Error::Resolve(
            "no active wsmr session generation; refusing to record cleanup state".into(),
        ));
    };

    let path = cleanup_path()?;
    let mut entries = files::read_cleanup_entries(&path)?;
    let mut added = false;
    for name in names {
        if crate::filter::keep_name(&name)
            && entries.insert(CleanupEntry {
                generation: generation.clone(),
                name,
            })
        {
            added = true;
        }
    }
    if added {
        files::write_cleanup_entries(&path, &entries)?;
    }
    Ok(())
}

/// End the current generation: restore the pre-session activation
/// environment, unset whatever this generation's cleanup list (plus
/// `always_cleanup`, minus `never_cleanup`) still calls for, then remove the
/// state files. Called once by `cleanup-env`.
pub fn end_generation(bus: &SessionBus) -> Result<()> {
    let _guard = lock()?;
    restore_and_clear_locked(bus)
}

/// Core of both [`begin_generation`]'s abandoned-state safety net and
/// [`end_generation`]: restore `env_pre` (if any) and unset whatever the
/// *current* generation's cleanup list calls for, then remove the state
/// files. Must be called with the lock already held. A no-op if no state is
/// currently on disk.
fn restore_and_clear_locked(bus: &SessionBus) -> Result<()> {
    let gen_path = generation_path()?;
    let cleanup_p = cleanup_path()?;
    let pre_p = env_pre_path()?;

    if !gen_path.exists() && !cleanup_p.exists() && !pre_p.exists() {
        return Ok(());
    }

    let current = read_generation(&gen_path)?;
    let listed: BTreeSet<String> = match &current {
        // Only ever act on entries tagged with the generation actually on
        // record — never on a stale/foreign generation's entries, and never
        // on anything if we can't identify the current generation at all.
        Some(g) => files::read_cleanup_entries(&cleanup_p)?
            .into_iter()
            .filter(|e| &e.generation == g)
            .map(|e| e.name)
            .collect(),
        None => BTreeSet::new(),
    };

    let env_pre = files::load_env(&pre_p)?;
    let pre_names: BTreeSet<String> = env_pre.keys().cloned().collect();
    let systemd_names: BTreeSet<String> = bus.systemd_vars()?.into_keys().collect();

    let mut to_unset = listed;
    for v in varnames::always_cleanup() {
        to_unset.insert(v.to_string());
    }
    let never = varnames::never_cleanup();
    to_unset.retain(|k| {
        !never.contains(k.as_str()) && systemd_names.contains(k) && !pre_names.contains(k)
    });

    if !to_unset.is_empty() {
        bus.unset_systemd_vars(&to_unset.into_iter().collect::<Vec<_>>())?;
    }
    if !env_pre.is_empty() {
        bus.set_systemd_vars(&env_pre)?;
    }

    for f in [&cleanup_p, &pre_p, &gen_path] {
        let _ = std::fs::remove_file(f);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::with_env;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn random_hex_id_is_16_hex() {
        let id = random_hex_id();
        assert_eq!(id.len(), 16);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn random_hex_id_is_not_constant() {
        // extremely unlikely to collide if the RNG source is actually used
        assert_ne!(random_hex_id(), random_hex_id());
    }

    fn rt_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("wsmr-state-{}-{}", std::process::id(), nanos))
    }

    #[test]
    fn append_cleanup_refuses_without_an_active_generation() {
        let rt = rt_dir();
        std::fs::create_dir_all(&rt).unwrap();
        with_env(&[("XDG_RUNTIME_DIR", Some(rt.to_str().unwrap()))], || {
            let err = append_cleanup(["FOO".to_string()]).unwrap_err();
            assert!(err.to_string().contains("no active"));
        });
        let _ = std::fs::remove_dir_all(&rt);
    }

    #[test]
    fn concurrent_appends_from_real_os_threads_lose_no_entries() {
        // Exercises the actual flock-based lock across genuine OS threads
        // (not just sequential calls): concurrent finalize/watcher-style
        // writers can't lose each other's cleanup entries.
        let rt = rt_dir();
        std::fs::create_dir_all(rt.join("wsmr")).unwrap();
        with_env(&[("XDG_RUNTIME_DIR", Some(rt.to_str().unwrap()))], || {
            fsutil::atomic_write_path(&generation_path().unwrap(), "concurrentgen0001\n").unwrap();

            let handles: Vec<_> = (0..24)
                .map(|i| std::thread::spawn(move || append_cleanup([format!("VAR_{i}")]).unwrap()))
                .collect();
            for h in handles {
                h.join().unwrap();
            }

            let entries = files::read_cleanup_entries(&cleanup_path().unwrap()).unwrap();
            assert_eq!(entries.len(), 24, "a concurrent append lost an entry");
            for i in 0..24 {
                assert!(entries.iter().any(|e| e.name == format!("VAR_{i}")));
            }
        });
        let _ = std::fs::remove_dir_all(&rt);
    }

    #[test]
    fn append_cleanup_tags_with_current_generation_and_preserves_others() {
        let rt = rt_dir();
        std::fs::create_dir_all(rt.join("wsmr")).unwrap();
        with_env(&[("XDG_RUNTIME_DIR", Some(rt.to_str().unwrap()))], || {
            // simulate a leftover entry from a different (older) generation
            let cleanup_p = cleanup_path().unwrap();
            let mut seed = BTreeSet::new();
            seed.insert(CleanupEntry {
                generation: "oldgen0000000000".into(),
                name: "STALE_VAR".into(),
            });
            files::write_cleanup_entries(&cleanup_p, &seed).unwrap();

            // establish "current" generation the way begin_generation would
            fsutil::atomic_write_path(&generation_path().unwrap(), "curgen0000000000\n").unwrap();

            append_cleanup(["NEW_VAR".to_string()]).unwrap();
            // appending the same name again must not duplicate it
            append_cleanup(["NEW_VAR".to_string()]).unwrap();

            let entries = files::read_cleanup_entries(&cleanup_p).unwrap();
            assert!(entries.contains(&CleanupEntry {
                generation: "curgen0000000000".into(),
                name: "NEW_VAR".into(),
            }));
            // the older generation's entry survives untouched
            assert!(entries.contains(&CleanupEntry {
                generation: "oldgen0000000000".into(),
                name: "STALE_VAR".into(),
            }));
            assert_eq!(entries.len(), 2);
        });
        let _ = std::fs::remove_dir_all(&rt);
    }

    #[test]
    fn append_cleanup_drops_invalid_names() {
        let rt = rt_dir();
        std::fs::create_dir_all(rt.join("wsmr")).unwrap();
        with_env(&[("XDG_RUNTIME_DIR", Some(rt.to_str().unwrap()))], || {
            fsutil::atomic_write_path(&generation_path().unwrap(), "gen0000000000000\n").unwrap();
            append_cleanup(["1BAD".to_string(), "OK".to_string()]).unwrap();
            let entries = files::read_cleanup_entries(&cleanup_path().unwrap()).unwrap();
            assert_eq!(entries.len(), 1);
            assert!(entries.iter().any(|e| e.name == "OK"));
        });
        let _ = std::fs::remove_dir_all(&rt);
    }

    #[test]
    fn read_generation_missing_file_is_none() {
        let rt = rt_dir();
        std::fs::create_dir_all(&rt).unwrap();
        assert_eq!(read_generation(&rt.join("nope")).unwrap(), None);
        let _ = std::fs::remove_dir_all(&rt);
    }

    #[test]
    fn read_generation_trims_and_rejects_empty() {
        let rt = rt_dir();
        std::fs::create_dir_all(&rt).unwrap();
        let p = rt.join("generation");
        std::fs::write(&p, "abc123\n").unwrap();
        assert_eq!(read_generation(&p).unwrap(), Some("abc123".to_string()));
        std::fs::write(&p, "\n").unwrap();
        assert_eq!(read_generation(&p).unwrap(), None);
        let _ = std::fs::remove_dir_all(&rt);
    }
}
