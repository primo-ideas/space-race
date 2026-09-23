//! How the game's binaries log.
//!
//! The crate exists for one rule that has already been got wrong once, and which is worth stating
//! in a single place: the level is set **explicitly**. `tracing_subscriber::fmt::init()` falls
//! back to `error` when the `env-filter` feature is on and `RUST_LOG` is unset, and Bevy turns
//! that feature on whenever the server is built together with the client. Built alone the server
//! logged normally; built with the workspace it was silent (see
//! [Architecture](../../docs/architecture.md#feature-unification)).
//!
//! The formatter is `tracing-subscriber`'s own rather than a layer of our own making: the server
//! wraps every connection and every lobby in a span and logs from inside it, so a line is only
//! readable with the span's fields attached to it. A layer that printed the message alone would
//! drop which session and which lobby a line came from, which is most of what a server log is for.
//!
//! The client does not call this. Bevy's `LogPlugin` installs a subscriber of its own before the
//! game starts, so a second one would be ignored, and on the web the logs go to the browser
//! console. The server is the only caller today.

use tracing_subscriber::EnvFilter;

/// What is logged when `RUST_LOG` says nothing.
pub const DEFAULT_FILTER: &str = "info";

/// Installs the subscriber for the whole process. Call it once, first thing in `main`.
///
/// Doing nothing when a subscriber is already in place, so a test that logs does not bring the
/// process down by calling this twice.
pub fn init() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter())
        .try_init();
}

/// What `RUST_LOG` asks for, or [`DEFAULT_FILTER`] when it says nothing — or something invalid,
/// which would otherwise silence the process over a typo.
pub fn filter() -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_filter_is_a_filter() {
        // A typo in the constant would not fail to compile; it would leave every binary silent.
        assert!(
            DEFAULT_FILTER
                .parse::<tracing_subscriber::filter::Directive>()
                .is_ok()
        );
    }

    #[test]
    fn a_bad_rust_log_falls_back_instead_of_silencing_everything() {
        // SAFETY: tests in one binary share the environment, and this is the only one that
        // touches RUST_LOG.
        unsafe { std::env::set_var("RUST_LOG", "=========") };
        let filter = filter();
        unsafe { std::env::remove_var("RUST_LOG") };

        assert_eq!(filter.to_string(), DEFAULT_FILTER);
    }
}
