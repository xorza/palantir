//! [`RealtimeClock`]: a host's frame time off the wall clock.

use std::time::{Duration, Instant};

/// Time since an [`Instant`] captured at construction, whose origin a window moves over the spans
/// it was hidden.
#[derive(Debug)]
pub(crate) struct RealtimeClock {
    origin: Instant,
}

impl RealtimeClock {
    /// A clock that starts now.
    pub(crate) fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }

    pub(crate) fn now(&self) -> Duration {
        self.origin.elapsed()
    }

    /// Advance the origin by `hidden`, so resuming from occlusion emits no giant animation `dt`.
    #[cfg_attr(
        not(feature = "winit"),
        expect(dead_code, reason = "only a window is hidden and shown again")
    )]
    pub(crate) fn skip(&mut self, hidden: Duration) {
        self.origin += hidden;
    }

    /// The wall-clock [`Instant`] at which frame time `at` falls due.
    #[cfg_attr(
        not(feature = "winit"),
        expect(dead_code, reason = "only the windowed host sleeps until a deadline")
    )]
    pub(crate) fn deadline(&self, at: Duration) -> Instant {
        self.origin + at
    }
}

#[cfg(test)]
mod tests {
    use crate::host::clock::RealtimeClock;
    use std::time::Duration;

    /// `skip` shifts the origin, so every deadline moves by exactly the skipped amount.
    #[test]
    fn deadline_and_skip_are_exact() {
        let mut c = RealtimeClock::new();
        let t0 = c.now();
        assert!(c.now() >= t0);
        let d0 = c.deadline(Duration::ZERO);
        let d1 = c.deadline(Duration::from_secs(1));
        assert_eq!(d1 - d0, Duration::from_secs(1));
        c.skip(Duration::from_millis(500));
        assert_eq!(c.deadline(Duration::ZERO) - d0, Duration::from_millis(500));
    }
}
