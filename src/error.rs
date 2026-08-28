//! Error types for wsmr.
//!
//! Library code returns [`Error`]; `main` adapts it to `anyhow` at the process
//! edge. See `docs/uwsm-core-analysis.md` §8.

use std::path::PathBuf;

/// Convenience result alias for the crate [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

/// Everything that can go wrong inside wsmr.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A filesystem operation failed, carrying the offending path for context.
    #[error("{path}: {source}")]
    Io {
        /// Path the operation targeted.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// A required environment variable was unset or empty.
    #[error("environment variable {0} is not set")]
    EnvMissing(String),

    /// A D-Bus call failed (only reachable on Linux at runtime).
    #[error("D-Bus: {0}")]
    Dbus(#[from] zbus::Error),

    /// A user-supplied argument was invalid.
    #[error("{0}")]
    InvalidArg(String),

    /// Something could not be resolved (e.g. the compositor command).
    #[error("could not resolve {0}")]
    Resolve(String),

    /// A feature whose milestone has not landed yet.
    #[error("{what} is not implemented yet ({milestone})")]
    NotImplemented {
        /// Milestone that will implement it (e.g. "M3").
        milestone: &'static str,
        /// Human description of the missing feature.
        what: &'static str,
    },

    /// Unit generation or removal was refused because one or more
    /// destinations are not verifiably owned by wsmr (a foreign or
    /// hand-edited file already occupies the path). See
    /// [`crate::units::generate::conflict_error`].
    #[error("{0}")]
    GenerationConflict(String),

    /// A `set`/`unset` activation-environment update touches both systemd
    /// and (for classic dbus-daemon only) D-Bus; this fires when one side
    /// succeeded and the other then failed, so the two are now inconsistent
    /// until the caller retries. See
    /// [`crate::sysd::dbus::SessionBus::set_systemd_vars`].
    #[error("{operation} succeeded on {applied}, but failed on {failed}: {source}")]
    PartialEnvUpdate {
        /// `"set"` or `"unset"`.
        operation: &'static str,
        /// Which side already has the change applied.
        applied: &'static str,
        /// Which side the failure left behind.
        failed: &'static str,
        /// The underlying D-Bus failure.
        #[source]
        source: Box<zbus::Error>,
    },
}

impl Error {
    /// Build [`Error::Io`] with path context.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Error::Io {
            path: path.into(),
            source,
        }
    }

    /// Build [`Error::NotImplemented`].
    pub fn todo(milestone: &'static str, what: &'static str) -> Self {
        Error::NotImplemented { milestone, what }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_messages() {
        let io = Error::io(
            "/x/y",
            std::io::Error::new(std::io::ErrorKind::NotFound, "missing"),
        );
        assert!(io.to_string().starts_with("/x/y: "));
        assert_eq!(
            Error::EnvMissing("HOME".into()).to_string(),
            "environment variable HOME is not set"
        );
        assert_eq!(Error::InvalidArg("bad".into()).to_string(), "bad");
        assert_eq!(
            Error::Resolve("comp".into()).to_string(),
            "could not resolve comp"
        );
        assert_eq!(
            Error::todo("M9", "warp drive").to_string(),
            "warp drive is not implemented yet (M9)"
        );
    }

    #[test]
    fn partial_env_update_names_both_sides() {
        let e = Error::PartialEnvUpdate {
            operation: "set",
            applied: "systemd",
            failed: "the D-Bus daemon",
            source: Box::new(zbus::Error::Failure("nope".into())),
        };
        let msg = e.to_string();
        assert!(msg.contains("set"));
        assert!(msg.contains("systemd"));
        assert!(msg.contains("the D-Bus daemon"));
        assert!(msg.contains("nope"));
    }
}
