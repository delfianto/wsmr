//! Application launching (`wsmr app`): desktop-entry parsing, `Exec` field
//! expansion, entry lookup, unit naming, and `systemd-run` assembly. See
//! `docs/uwsm-core-analysis.md` §6 and `REFERENCE.md` §13.

pub mod daemon;
pub mod entry;
pub mod field;
pub mod find;
pub mod launch;
pub mod naming;
pub mod terminal;
