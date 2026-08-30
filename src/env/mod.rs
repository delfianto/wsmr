//! Environment bootstrapping engine: the pure delta computation, env-file
//! (de)serialization, and shell `env -0` dump parsing that drive
//! prepare-env / finalize / cleanup-env.

pub mod delta;
pub mod dump;
pub mod files;
