//! [`WakeSignal`]: the wake a host with no event loop keeps for its next frame.

use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Instant;

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

    /// Block until a wake waits or `deadline` passes; whether one waits.
    pub(crate) fn wait(&self, deadline: Instant) -> bool {
        let timeout = deadline.saturating_duration_since(Instant::now());
        let (woken, _) = self
            .wakes
            .wait_timeout_while(self.woken(), timeout, |woken| !*woken)
            .unwrap_or_else(PoisonError::into_inner);
        *woken
    }

    fn woken(&self) -> std::sync::MutexGuard<'_, bool> {
        self.woken.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    use crate::host::wake_signal::WakeSignal;

    /// A wake from another thread ends a wait at once and waits to be taken; with none, the wait
    /// ends at its deadline.
    #[test]
    fn a_wake_ends_a_wait_and_waits_to_be_taken() {
        let signal = Arc::new(WakeSignal::default());
        assert!(!signal.wait(Instant::now() + Duration::from_millis(5)));
        let waker = signal.waker();
        let woke = thread::spawn(move || waker.wake());
        assert!(signal.wait(Instant::now() + Duration::from_secs(10)));
        woke.join().unwrap();
        assert!(signal.is_pending());
        assert!(signal.take());
        assert!(!signal.is_pending() && !signal.take());
    }
}
