//! Assert that a closure panics *for the stated reason*.
//!
//! `catch_unwind(..).is_err()` accepts any panic: an index error in the
//! fixture, an unprimed frame, an overflow. [`assert_panics_with`] checks
//! the message, and keeps the expected panic off the test output.

use std::any::Any;
use std::cell::Cell;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Once;

thread_local! {
    /// Whether the panic hook stays quiet on this thread. Per thread
    /// because libtest runs tests in parallel, and the hook is
    /// process-wide: swapping the hook itself per call would race.
    static QUIET: Cell<bool> = const { Cell::new(false) };
}

static INSTALL: Once = Once::new();

/// Run `f`, and assert that it panics with a message containing
/// `fragment`.
///
/// # Panics
///
/// Panics when `f` returns normally, or panics with a message that does
/// not contain `fragment`.
#[track_caller]
pub(crate) fn assert_panics_with<R>(fragment: &str, f: impl FnOnce() -> R) {
    INSTALL.call_once(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            if !QUIET.with(Cell::get) {
                previous(info);
            }
        }));
    });
    // Restored rather than cleared, so a nested probe leaves an outer
    // one quiet.
    let was_quiet = QUIET.with(|quiet| quiet.replace(true));
    let result = panic::catch_unwind(AssertUnwindSafe(f));
    QUIET.with(|quiet| quiet.set(was_quiet));
    let Err(payload) = result else {
        panic!("expected a panic containing {fragment:?}, but the closure returned");
    };
    let message = message_of(payload.as_ref());
    assert!(
        message.contains(fragment),
        "expected a panic containing {fragment:?}, got {message:?}",
    );
}

fn message_of(payload: &(dyn Any + Send)) -> &str {
    if let Some(message) = payload.downcast_ref::<&str>() {
        message
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message
    } else {
        "<non-string panic payload>"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_message_passes_and_others_fail() {
        assert_panics_with("needle", || panic!("hay needle hay"));
        assert_panics_with("formatted 7", || panic!("formatted {}", 7));
        assert_panics_with(r#"containing "needle", got "hay""#, || {
            assert_panics_with("needle", || panic!("hay"));
        });
        assert_panics_with("but the closure returned", || {
            assert_panics_with("needle", || 1);
        });
    }
}
