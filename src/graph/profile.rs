//! No-op profiling stubs — all functions are empty so the compiler
//! can elide the calls entirely under LTO. Keeping the call sites
//! means we can re-enable profiling later by restoring the bodies
//! (see git history for the real implementation).
//!
//! To re-enable: restore the `BatchProfile` struct and the thread-local
//! accumulator in this file.

use std::time::Duration;

/// Begin a profiling session on the current thread.
pub fn begin_batch(_label: String) {}

/// Record elapsed time under a named category.
pub fn record(_label: &'static str, _elapsed: Duration) {}

/// Time a closure and record it.
pub fn time<T>(_label: &'static str, f: impl FnOnce() -> T) -> T {
    f()
}

/// Log the summary and drop the session.
pub fn end_batch() {}
