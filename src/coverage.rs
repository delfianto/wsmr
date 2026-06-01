//! Coverage-only profile flush.
//!
//! wsmr's lifecycle ends most of its processes with `exec()` (the compositor
//! anchor in [`session::start`](crate::session::start), the readiness/finalize
//! `systemd-notify`, the single-instance `systemd-run` in
//! [`app::launch`](crate::app::launch)). Under `cargo llvm-cov` the LLVM
//! profiling runtime writes its `.profraw` from an `atexit` hook — but `exec()`
//! replaces the process image *before* `atexit` runs, so every line that
//! executed before the `exec()` would be reported as uncovered.
//!
//! [`flush_before_exec`] writes the in-memory profile to disk right before each
//! `exec()`, recovering that coverage. It is compiled in **only** under
//! `cfg(coverage)` (which `cargo llvm-cov` sets together with
//! `-Cinstrument-coverage`); in every normal/release build it is an empty inline
//! no-op with no FFI and no link dependency on the profiling runtime.

/// Flush the in-memory coverage profile to disk. A no-op unless built under
/// `cargo llvm-cov` (`cfg(coverage)`); call immediately before an `exec()`.
#[inline]
pub fn flush_before_exec() {
    #[cfg(coverage)]
    {
        // SAFETY: `__llvm_profile_write_file` is provided by the LLVM profiling
        // runtime, which is linked whenever `-Cinstrument-coverage` is active —
        // and cargo-llvm-cov sets that flag together with `cfg(coverage)`, so the
        // symbol is guaranteed present here. It takes no arguments and is safe to
        // call repeatedly.
        unsafe extern "C" {
            fn __llvm_profile_write_file() -> core::ffi::c_int;
        }
        unsafe {
            __llvm_profile_write_file();
        }
    }
}
