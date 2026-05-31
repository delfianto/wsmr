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
