//! systemd / logind / D-Bus integration over the **session** and **system**
//! buses (blocking zbus). Scaffolded in M1: compiles on any platform but only
//! functions against a live bus on Linux. See `REFERENCE.md` §8.1.

pub mod dbus;
