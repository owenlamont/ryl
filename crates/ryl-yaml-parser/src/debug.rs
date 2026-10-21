//! Debugging helpers.
//!
//! Debugging is governed by two conditions:
//!   1. The `debug_prints` feature. Debugging code is not emitted unless that feature is enabled.
//!   2. The local [`ENABLED`] constant below. Flip it to `true` in a local build when you want
//!      the debug helpers to print.

// If a debug build, use stuff in the debug submodule.
#[cfg(feature = "debug_prints")]
pub use debug::enabled;

// Otherwise, just export dummies for publicly visible functions.
/// Evaluates to nothing.
#[cfg(not(feature = "debug_prints"))]
macro_rules! debug_print {
    ($($arg:tt)*) => {{}};
}

#[cfg(feature = "debug_prints")]
#[macro_use]
#[allow(clippy::module_inception)]
mod debug {
    /// Local compile-time toggle for debug prints.
    ///
    /// This avoids runtime environment-variable reads while still keeping the output opt-in.
    const ENABLED: bool = false;

    /// If debugging is [`enabled`], print the format string on the error output.
    macro_rules! debug_print {
    ($($arg:tt)*) => {{
        if $crate::debug::enabled() {
            std::eprintln!($($arg)*)
        }
    }};
    }

    /// Return whether debugging features are enabled in this execution.
    pub fn enabled() -> bool {
        ENABLED
    }
}
