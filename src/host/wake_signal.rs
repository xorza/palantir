//! [`WakeSignal`]: the wake a host with no event loop keeps for its next frame.

use crate::common::sync::{CondvarExt, MutexExt};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use crate::window::waker::Waker;

/// A wake from a [`Waker`], kept until a frame takes it: the [`Harness`](crate::Harness)'s and
/// the [`OffscreenHost`](crate::OffscreenHost)'s, which run no event loop for a wake to start.
#[derive(Debug, Default)]
pub(crate) struct WakeSignal {
    woken: Mutex<bool>,
    wakes: Condvar,
}

impl WakeSignal {
    /// A waker that sets this signal.
    pub(crate) fn waker(self: &Arc<Self>) -> Waker {
        let signal = Arc::clone(self);
        Waker::new(move || signal.wake())
    }

    fn wake(&self) {
        *self.woken() = true;
        self.wakes.notify_all();
    }

    /// Whether a wake waits for a frame.
    pub(crate) fn is_pending(&self) -> bool {
        *self.woken()
    }

    /// Take the waiting wake, if one waits.
    pub(crate) fn take(&self) -> bool {
        std::mem::take(&mut *self.woken())
    }

    /// Block until a wake waits, for `timeout` at most; whether one waits.
    pub(crate) fn wait(&self, timeout: Duration) -> bool {
        let woken = self
            .wakes
            .wait_timeout_while_unpoisoned(self.woken(), timeout, |woken| !*woken);
        *woken
    }

    fn woken(&self) -> std::sync::MutexGuard<'_, bool> {
        self.woken.lock_unpoisoned()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    use crate::host::wake_signal::WakeSignal;

    /// A wake from another thread ends a wait at once and waits to be taken; with none, the wait
    /// ends at its timeout.
    #[test]
    fn a_wake_ends_a_wait_and_waits_to_be_taken() {
        let signal = Arc::new(WakeSignal::default());
        assert!(!signal.wait(Duration::from_millis(5)));
        let waker = signal.waker();
        let woke = thread::spawn(move || waker.wake());
        assert!(signal.wait(Duration::from_secs(10)));
        woke.join().unwrap();
        assert!(signal.is_pending());
        assert!(signal.take());
        assert!(!signal.is_pending() && !signal.take());
    }
}
