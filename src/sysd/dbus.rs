//! Typed blocking D-Bus client, porting `uwsm/uwsm/dbus.py`.
//!
//! **M1 scaffold:** the proxies and wrapper compile everywhere, but connecting
//! and calling only works against a live session/system bus on Linux. There is
//! no integration test on macOS. See `REFERENCE.md` §8.1.

// The D-Bus `Notify` method legitimately takes many parameters.
#![allow(clippy::too_many_arguments)]

use crate::error::Result;
use crate::filter;
use std::collections::{BTreeMap, HashMap};
use std::thread::sleep;
use std::time::{Duration, Instant};
use zbus::proxy;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Type};

use serde::Deserialize;

/// One entry of `ListUnitsByPatterns` (`a(ssssssouso)`).
#[derive(Debug, Clone, Type, Deserialize)]
pub struct UnitInfo {
    /// Primary unit name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Load state (loaded/error/…).
    pub load_state: String,
    /// Active state (active/inactive/activating/…).
    pub active_state: String,
    /// Sub state.
    pub sub_state: String,
    /// Unit it is following, if any.
    pub following: String,
    /// Unit object path.
    pub unit_path: OwnedObjectPath,
    /// Queued job id (0 if none).
    pub job_id: u32,
    /// Queued job type.
    pub job_type: String,
    /// Queued job object path.
    pub job_path: OwnedObjectPath,
}

/// One entry of `ListJobs` (`a(usssoo)`).
#[derive(Debug, Clone, Type, Deserialize)]
pub struct JobInfo {
    /// Job id.
    pub id: u32,
    /// Unit the job acts on.
    pub unit: String,
    /// Job type.
    pub job_type: String,
    /// Job state.
    pub state: String,
    /// Job object path.
    pub job_path: OwnedObjectPath,
    /// Unit object path.
    pub unit_path: OwnedObjectPath,
}

/// One entry of logind `ListSessions` (`a(susso)`).
#[derive(Debug, Clone, Type, Deserialize)]
pub struct SessionInfo {
    /// Session id.
    pub session_id: String,
    /// Owner uid.
    pub uid: u32,
    /// Owner user name.
    pub user: String,
    /// Seat id.
    pub seat: String,
    /// Session object path.
    pub path: OwnedObjectPath,
}

#[proxy(
    interface = "org.freedesktop.systemd1.Manager",
    default_service = "org.freedesktop.systemd1",
    default_path = "/org/freedesktop/systemd1"
)]
pub trait SystemdManager {
    /// Reload the manager configuration.
    fn reload(&self) -> zbus::Result<()>;
    /// Get a unit's object path by name.
    fn get_unit(&self, name: &str) -> zbus::Result<OwnedObjectPath>;
    /// Stop a unit, returning the queued job path.
    fn stop_unit(&self, name: &str, mode: &str) -> zbus::Result<OwnedObjectPath>;
    /// Set activation-environment assignments (`KEY=VALUE`).
    fn set_environment(&self, assignments: Vec<String>) -> zbus::Result<()>;
    /// Unset activation-environment variables by name.
    fn unset_environment(&self, names: Vec<String>) -> zbus::Result<()>;
    /// List units matching states and glob patterns.
    fn list_units_by_patterns(
        &self,
        states: Vec<String>,
        patterns: Vec<String>,
    ) -> zbus::Result<Vec<UnitInfo>>;
    /// List queued jobs.
    fn list_jobs(&self) -> zbus::Result<Vec<JobInfo>>;
    /// The manager activation environment (`KEY=VALUE` array).
    #[zbus(property)]
    fn environment(&self) -> zbus::Result<Vec<String>>;
}

#[proxy(
    interface = "org.freedesktop.systemd1.Unit",
    default_service = "org.freedesktop.systemd1"
)]
pub trait SystemdUnit {
    /// Unit id (e.g. `dbus-broker.service`).
    #[zbus(property)]
    fn id(&self) -> zbus::Result<String>;
    /// Active state (active/inactive/activating/…).
    #[zbus(property, name = "ActiveState")]
    fn active_state(&self) -> zbus::Result<String>;
}

#[proxy(
    interface = "org.freedesktop.systemd1.Service",
    default_service = "org.freedesktop.systemd1"
)]
pub trait SystemdService {
    /// Who may send readiness notifications (`all`/`exec`/`main`/`none`).
    #[zbus(property, name = "NotifyAccess")]
    fn notify_access(&self) -> zbus::Result<String>;
    /// Start timeout in microseconds.
    #[zbus(property, name = "TimeoutStartUSec")]
    fn timeout_start_usec(&self) -> zbus::Result<u64>;
}

#[proxy(
    interface = "org.freedesktop.login1.Manager",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1"
)]
pub trait LogindManager {
    /// List login sessions.
    fn list_sessions(&self) -> zbus::Result<Vec<SessionInfo>>;
    /// Get a session's object path by id.
    fn get_session(&self, id: &str) -> zbus::Result<OwnedObjectPath>;
}

#[proxy(
    interface = "org.freedesktop.login1.Session",
    default_service = "org.freedesktop.login1"
)]
pub trait LogindSession {
    /// Virtual terminal number.
    #[zbus(property, name = "VTNr")]
    fn vtnr(&self) -> zbus::Result<u32>;
    /// Session leader PID.
    #[zbus(property)]
    fn leader(&self) -> zbus::Result<u32>;
    /// Whether the session is remote (not local).
    #[zbus(property, name = "Remote")]
    fn remote(&self) -> zbus::Result<bool>;
}

#[proxy(
    interface = "org.freedesktop.DBus",
    default_service = "org.freedesktop.DBus",
    default_path = "/org/freedesktop/DBus"
)]
pub trait DBusDaemon {
    /// Merge variables into the D-Bus activation environment.
    fn update_activation_environment(&self, env: HashMap<String, String>) -> zbus::Result<()>;
}

#[proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
pub trait Notifications {
    /// Post a desktop notification.
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;
}

/// Connection to the **session** bus with the systemd-user / D-Bus / notify
/// surface.
pub struct SessionBus {
    conn: zbus::blocking::Connection,
}

impl SessionBus {
    /// Connect to the session bus.
    pub fn connect() -> Result<Self> {
        Ok(Self {
            conn: zbus::blocking::Connection::session()?,
        })
    }

    fn manager(&self) -> Result<SystemdManagerProxyBlocking<'_>> {
        Ok(SystemdManagerProxyBlocking::new(&self.conn)?)
    }

    fn dbus(&self) -> Result<DBusDaemonProxyBlocking<'_>> {
        Ok(DBusDaemonProxyBlocking::new(&self.conn)?)
    }

    /// Read the systemd activation environment, filtered to valid names.
    pub fn systemd_vars(&self) -> Result<BTreeMap<String, String>> {
        let mut map = BTreeMap::new();
        for assignment in self.manager()?.environment()? {
            if let Some((k, v)) = assignment.split_once('=')
                && filter::keep_name(k)
            {
                map.insert(k.to_string(), v.to_string());
            }
        }
        Ok(map)
    }

    /// Whether the running D-Bus daemon is `dbus-broker` (which shares the
    /// systemd activation environment, so the separate D-Bus update is skipped).
    pub fn is_dbus_broker(&self) -> Result<bool> {
        let path = self.manager()?.get_unit("dbus.service")?;
        let unit = SystemdUnitProxyBlocking::builder(&self.conn)
            .path(path)?
            .build()?;
        Ok(unit.id()? == "dbus-broker.service")
    }

    /// Export variables to the systemd (and, for classic dbus-daemon, the D-Bus)
    /// activation environment. Ports `set_systemd_vars` (`main.py:917`).
    pub fn set_systemd_vars(&self, vars: &BTreeMap<String, String>) -> Result<()> {
        let assignments: Vec<String> = vars.iter().map(|(k, v)| format!("{k}={v}")).collect();
        self.manager()?.set_environment(assignments)?;
        if !self.is_dbus_broker()? {
            let map: HashMap<String, String> = vars.clone().into_iter().collect();
            self.dbus()?.update_activation_environment(map)?;
        }
        Ok(())
    }

    /// Unset variables from the systemd (and, for classic dbus-daemon, the
    /// D-Bus) activation environment. Ports `unset_systemd_vars` (`main.py:977`).
    pub fn unset_systemd_vars(&self, names: &[String]) -> Result<()> {
        if !self.is_dbus_broker()? {
            let map: HashMap<String, String> =
                names.iter().map(|n| (n.clone(), String::new())).collect();
            self.dbus()?.update_activation_environment(map)?;
        }
        self.manager()?.unset_environment(names.to_vec())?;
        Ok(())
    }

    /// Reload the systemd user manager.
    pub fn reload(&self) -> Result<()> {
        self.manager()?.reload()?;
        Ok(())
    }

    /// List units matching states + glob patterns.
    pub fn list_units_by_patterns(
        &self,
        states: &[&str],
        patterns: &[&str],
    ) -> Result<Vec<UnitInfo>> {
        let states = states.iter().map(|s| s.to_string()).collect();
        let patterns = patterns.iter().map(|s| s.to_string()).collect();
        Ok(self.manager()?.list_units_by_patterns(states, patterns)?)
    }

    /// Stop a unit, returning the queued job path.
    pub fn stop_unit(&self, name: &str, mode: &str) -> Result<OwnedObjectPath> {
        Ok(self.manager()?.stop_unit(name, mode)?)
    }

    /// List queued jobs.
    pub fn list_jobs(&self) -> Result<Vec<JobInfo>> {
        Ok(self.manager()?.list_jobs()?)
    }

    /// Post a simple desktop notification.
    pub fn notify(&self, summary: &str, body: &str) -> Result<u32> {
        let n = NotificationsProxyBlocking::new(&self.conn)?;
        Ok(n.notify(
            "wsmr",
            0,
            "applications-system",
            summary,
            body,
            Vec::new(),
            HashMap::new(),
            -1,
        )?)
    }

    fn service(&self, unit: &str) -> Result<SystemdServiceProxyBlocking<'_>> {
        let path = self.manager()?.get_unit(unit)?;
        Ok(SystemdServiceProxyBlocking::builder(&self.conn)
            .path(path)?
            .build()?)
    }

    /// `NotifyAccess` of a service unit.
    pub fn service_notify_access(&self, unit: &str) -> Result<String> {
        Ok(self.service(unit)?.notify_access()?)
    }

    /// `TimeoutStartUSec` of a service unit (microseconds).
    pub fn service_timeout_start_usec(&self, unit: &str) -> Result<u64> {
        Ok(self.service(unit)?.timeout_start_usec()?)
    }

    /// Name of the active/activating `wayland-wm@*.service`, if any.
    pub fn active_wm_unit(&self) -> Result<Option<String>> {
        let units =
            self.list_units_by_patterns(&["active", "activating"], &["wayland-wm@*.service"])?;
        Ok(units.into_iter().next().map(|u| u.name))
    }

    /// Wait until the job queue no longer contains `job` (poll). Used after
    /// `reload`/`stop_unit`.
    pub fn wait_for_job(&self, job: &OwnedObjectPath) -> Result<()> {
        loop {
            if !self.list_jobs()?.iter().any(|j| &j.job_path == job) {
                return Ok(());
            }
            sleep(Duration::from_millis(100));
        }
    }
}

/// Connection to the **system** bus with the logind surface.
pub struct SystemBus {
    conn: zbus::blocking::Connection,
}

impl SystemBus {
    /// Connect to the system bus.
    pub fn connect() -> Result<Self> {
        Ok(Self {
            conn: zbus::blocking::Connection::system()?,
        })
    }

    fn manager(&self) -> Result<LogindManagerProxyBlocking<'_>> {
        Ok(LogindManagerProxyBlocking::new(&self.conn)?)
    }

    /// List login sessions.
    pub fn list_sessions(&self) -> Result<Vec<SessionInfo>> {
        Ok(self.manager()?.list_sessions()?)
    }

    fn session(&self, id: &str) -> Result<LogindSessionProxyBlocking<'_>> {
        let path = self.manager()?.get_session(id)?;
        Ok(LogindSessionProxyBlocking::builder(&self.conn)
            .path(path)?
            .build()?)
    }

    /// VT number of a login session.
    pub fn session_vtnr(&self, id: &str) -> Result<u32> {
        Ok(self.session(id)?.vtnr()?)
    }

    /// Leader PID of a login session.
    pub fn session_leader(&self, id: &str) -> Result<u32> {
        Ok(self.session(id)?.leader()?)
    }

    /// Whether a login session is remote.
    pub fn session_remote(&self, id: &str) -> Result<bool> {
        Ok(self.session(id)?.remote()?)
    }

    /// Find the login session on a given VT for the current user. Returns
    /// `(session_id, seat_id)`. Ports `get_session_by_vt` (`main.py:2592`).
    pub fn session_by_vt(&self, vtnr: u32) -> Result<Option<(String, String)>> {
        let uid = current_uid();
        for s in self.list_sessions()? {
            if s.uid != uid {
                continue;
            }
            if let Ok(v) = self.session_vtnr(&s.session_id)
                && v == vtnr
            {
                return Ok(Some((s.session_id, s.seat)));
            }
        }
        Ok(None)
    }

    fn systemd(&self) -> Result<SystemdManagerProxyBlocking<'_>> {
        Ok(SystemdManagerProxyBlocking::new(&self.conn)?)
    }

    fn unit_active_state(&self, unit: &str) -> Result<Option<String>> {
        let path = match self.systemd()?.get_unit(unit) {
            Ok(p) => p,
            // unit not loaded yet → treat as not-active
            Err(_) => return Ok(None),
        };
        let u = SystemdUnitProxyBlocking::builder(&self.conn)
            .path(path)?
            .build()?;
        Ok(Some(u.active_state()?))
    }

    /// Poll the system `graphical.target` (or any unit) until its `ActiveState`
    /// is in `states`, or `timeout` elapses. Ports the gst gate of `start`
    /// (`main.py:4754`).
    pub fn wait_for_unit(&self, unit: &str, states: &[&str], timeout: Duration) -> Result<bool> {
        let start = Instant::now();
        loop {
            if let Some(state) = self.unit_active_state(unit)?
                && states.contains(&state.as_str())
            {
                return Ok(true);
            }
            if start.elapsed() >= timeout {
                return Ok(false);
            }
            sleep(Duration::from_millis(500));
        }
    }
}

/// Extract the compositor id from a `wayland-wm@<id>.service` unit name.
/// Ports `extract_wm_id` (`main.py:1178`).
pub fn extract_wm_id(unit: &str) -> Option<String> {
    unit.strip_prefix("wayland-wm@")
        .and_then(|s| s.strip_suffix(".service"))
        .map(str::to_string)
}

// SAFETY: getuid() is always safe — it cannot fail and touches no memory.
fn current_uid() -> u32 {
    unsafe { libc::getuid() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_wm_id_parses() {
        assert_eq!(
            extract_wm_id("wayland-wm@sway.service").as_deref(),
            Some("sway")
        );
        assert_eq!(
            extract_wm_id("wayland-wm@my\\x2dcomp.service").as_deref(),
            Some("my\\x2dcomp")
        );
        assert_eq!(extract_wm_id("other.service"), None);
    }
}
