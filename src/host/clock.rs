//! The per-frame time source, injected into a host's [`WindowDriver`](crate::host::window_driver::WindowDriver): [`RealtimeClock`] off the wall clock for windows, [`FixedClock`] off a caller-controlled value for reproducible offscreen renders.

use std::fmt;
use std::time::{Duration, Instant};

/// Source of the per-frame monotonic timestamp, read once per frame. `skip` / `deadline` serve the on-screen path and default to no-ops.
pub trait Clock: fmt::Debug {
    /// The current monotonic time.
    fn now(&self) -> Duration;

    /// Advance the origin by `hidden` so resuming from occlusion doesn't emit one giant animation `dt`; a fixed clock ignores it.
    fn skip(&mut self, hidden: Duration) {
        let _ = hidden;
    }

    /// The wall-clock [`Instant`] at which frame-time `at` falls due, for `WaitUntil`; `None` without a wall-time origin.
    fn deadline(&self, at: Duration) -> Option<Instant> {
        let _ = at;
        None
    }
}

/// Wall-clock source for on-screen windows: time since an [`Instant`] captured at construction.
#[derive(Debug)]
pub struct RealtimeClock {
    origin: Instant,
}

impl RealtimeClock {
    /// A clock that starts now.
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for RealtimeClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for RealtimeClock {
    fn now(&self) -> Duration {
        self.origin.elapsed()
    }

    fn skip(&mut self, hidden: Duration) {
        self.origin += hidden;
    }

    fn deadline(&self, at: Duration) -> Option<Instant> {
        Some(self.origin + at)
    }
}

/// Deterministic source: [`Clock::now`] moves only when the owner [`advance`](Self::advance)s it, so every frame samples the same phase (the spinner at angle 0).
#[derive(Debug, Default)]
pub struct FixedClock {
    now: Duration,
}

impl FixedClock {
    /// A fixed clock reading `now`.
    pub const fn new(now: Duration) -> Self {
        Self { now }
    }

    /// Moves the fixed clock forward by `dt`.
    pub fn advance(&mut self, dt: Duration) {
        self.now += dt;
    }
}

impl Clock for FixedClock {
    fn now(&self) -> Duration {
        self.now
    }
}

#[cfg(test)]
mod tests {
    use super::{Clock, FixedClock, RealtimeClock};
    use std::time::Duration;

    #[test]
    fn fixed_clock_holds_and_advances() {
        let mut c = FixedClock::new(Duration::from_millis(500));
        assert_eq!(c.now(), Duration::from_millis(500));
        assert_eq!(c.now(), Duration::from_millis(500));
        c.advance(Duration::from_millis(250));
        assert_eq!(c.now(), Duration::from_millis(750));
        c.skip(Duration::from_secs(10));
        assert_eq!(c.now(), Duration::from_millis(750));
        assert_eq!(c.deadline(Duration::from_secs(1)), None);
    }

    #[test]
    fn realtime_clock_deadline_and_skip_are_exact() {
        let mut c = RealtimeClock::new();
        let t0 = c.now();
        assert!(c.now() >= t0);
        let d0 = c.deadline(Duration::ZERO).unwrap();
        let d1 = c.deadline(Duration::from_secs(1)).unwrap();
        assert_eq!(d1 - d0, Duration::from_secs(1));
        // `skip` shifts the origin, so every deadline moves by exactly the skipped amount.
        c.skip(Duration::from_millis(500));
        let d0_after = c.deadline(Duration::ZERO).unwrap();
        assert_eq!(d0_after - d0, Duration::from_millis(500));
    }
}
