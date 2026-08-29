//! Test-only helpers. Compiled only under `cfg(test)`.

use std::sync::{Mutex, MutexGuard, OnceLock};

/// A guaranteed-nonexistent path, for pointing `XDG_DATA_DIRS`/
/// `XDG_CONFIG_DIRS` away from the host's real system directories in tests
/// that need a fully isolated desktop-entry/config hierarchy.
///
/// **Not** an empty string: `crate::util::xdg::data_dirs`/`config_dirs`
/// treat `""` as *unset* (by design — see `util::xdg`'s own tests) and fall
/// back to `/usr/share`+co, so a test that sets these to `""` silently pulls
/// in the host's real desktop entries instead of excluding them (this is
/// exactly what made `app::terminal`'s tests host-dependent). A nonexistent
/// absolute path is a real, non-empty value, so it's honored as "search
/// here" — and every reader in this crate already treats a missing
/// directory as simply empty, so nothing needs to exist on disk at this
/// path.
pub const NO_XDG_DIRS: &str = "/nonexistent-wsmr-test-xdg-dirs";

/// Process-wide lock serializing tests that mutate `std::env`. `set_var` /
/// `remove_var` are process-global (and `unsafe` in edition 2024), so env-driven
/// tests must not run concurrently with each other.
fn env_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    // A poisoned lock is fine here — we only use it for mutual exclusion.
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Run `f` with `vars` applied to the environment (a `None` value unsets the
/// var), serialized against other `with_env` calls and restored afterward —
/// even on panic.
pub fn with_env<T>(vars: &[(&str, Option<&str>)], f: impl FnOnce() -> T) -> T {
    let _guard = env_lock();

    // snapshot prior values so we can restore exactly
    let prior: Vec<(String, Option<String>)> = vars
        .iter()
        .map(|(k, _)| ((*k).to_string(), std::env::var(*k).ok()))
        .collect();
    apply(vars.iter().map(|(k, v)| (*k, *v)));

    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));

    apply(prior.iter().map(|(k, v)| (k.as_str(), v.as_deref())));

    match out {
        Ok(v) => v,
        Err(p) => std::panic::resume_unwind(p),
    }
}

fn apply<'a>(vars: impl Iterator<Item = (&'a str, Option<&'a str>)>) {
    for (k, v) in vars {
        // SAFETY: serialized by `env_lock`; no other test thread touches env
        // while this guard is held.
        unsafe {
            match v {
                Some(val) => std::env::set_var(k, val),
                None => std::env::remove_var(k),
            }
        }
    }
}
