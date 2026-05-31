//! `cleanup-env`: scrub session-added variables and restore the pre-session
//! activation environment. Ports `cleanup_env` (`main.py:2922`).
//! See `REFERENCE.md` §6.

use crate::env::files;
use crate::error::Result;
use crate::session::runtime_path;
use crate::sysd::dbus::SessionBus;
use crate::varnames;
use std::collections::BTreeSet;

/// Scrub `(cleanup_list ∪ always_cleanup) − never_cleanup ∩ systemd − env_pre`,
/// restore `env_pre`, and remove the runtime files.
pub fn cleanup_env() -> Result<()> {
    let bus = SessionBus::connect()?;
    let cleanup_path = runtime_path("env_cleanup.list")?;
    let env_pre_path = runtime_path("env_pre")?;
    let session_conf = runtime_path("env_session.conf")?;

    let listed = files::read_cleanup(&cleanup_path)?;
    let systemd_names: BTreeSet<String> = bus.systemd_vars()?.into_keys().collect();
    let env_pre = files::load_env(&env_pre_path)?;
    let pre_names: BTreeSet<String> = env_pre.keys().cloned().collect();

    let always = varnames::always_cleanup();
    let never = varnames::never_cleanup();

    let mut to_unset: BTreeSet<String> = listed;
    for v in &always {
        to_unset.insert(v.to_string());
    }
    to_unset.retain(|k| {
        !never.contains(k.as_str()) && systemd_names.contains(k) && !pre_names.contains(k)
    });

    if !to_unset.is_empty() {
        let names: Vec<String> = to_unset.into_iter().collect();
        bus.unset_systemd_vars(&names)?;
    }
    if !env_pre.is_empty() {
        bus.set_systemd_vars(&env_pre)?;
    }

    for f in [&cleanup_path, &env_pre_path, &session_conf] {
        let _ = std::fs::remove_file(f);
    }
    Ok(())
}
