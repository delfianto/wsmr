//! Test-only helpers. Compiled only under `cfg(test)`.

use std::sync::{Mutex, MutexGuard, OnceLock};

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
