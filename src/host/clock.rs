//! The per-frame time source, injected when a host builds its
//! [`WindowDriver`](crate::host::window_driver::WindowDriver).
//!
//! One trait, [`Clock`], resolves the monotonic timestamp fed to every
//! [`FrameStamp`](crate::ui::frame::FrameStamp) and sampled by paint /
//! value animations. Two implementations cover the two ways frames are
//! driven: [`RealtimeClock`] off the wall clock for on-screen windows, and
//! [`FixedClock`] off a caller-controlled value for reproducible offscreen
//! renders — golden tests, thumbnails, server-side compositing. Because the
//! choice is an injected dependency rather than a branch inside the
//! renderer, the same pipeline drives both.

use std::time::{Duration, Instant};

/// Source of the per-frame monotonic timestamp. `now()` is read once per
/// frame and handed to `FrameStamp`, so animations advance by the delta
/// between successive reads.
///
/// `skip` / `deadline` support the on-screen path (occlusion pause, present
/// scheduling) and default to no-ops so a headless clock only has to
/// implement `now`.
pub trait Clock: std::fmt::Debug {
    /// Monotonic time since this clock's origin.
    fn now(&self) -> Duration;

    /// Advance the origin forward by `hidden` so resuming from occlusion
    /// doesn't emit one giant animation `dt`. The wall clock shifts its
    /// anchor; a fixed clock ignores it (headless never occludes).
    fn skip(&mut self, hidden: Duration) {
        let _ = hidden;
    }

    /// The wall-clock [`Instant`] at which frame-time `at` (measured from
    /// the origin, as carried by `FrameReport::repaint_after`) falls due —
    /// for a host's `WaitUntil`. `None` when the clock has no wall-time
    /// origin: an offscreen render has no real wait to schedule.
    fn deadline(&self, at: Duration) -> Option<Instant> {
        let _ = at;
        None
    }
}

/// Wall-clock time source: [`Clock::now`] is the elapsed time since an
/// [`Instant`] origin captured at construction. The clock on-screen windows
/// use.
#[derive(Debug)]
pub struct RealtimeClock {
    origin: Instant,
}

impl RealtimeClock {
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

/// Deterministic time source: [`Clock::now`] returns a fixed value that
/// only moves when the owner [`advance`](Self::advance)s it. Every frame
/// then samples the same phase, so an offscreen render is reproducible —
/// `FixedClock::new(Duration::ZERO)` paints animations at their start phase
/// (the spinner at angle 0). The `skip` / `deadline` no-ops apply: a fixed
/// clock never occludes and schedules no real waits.
#[derive(Debug, Default)]
pub struct FixedClock {
    now: Duration,
}

impl FixedClock {
    pub fn new(now: Duration) -> Self {
        Self { now }
    }

    /// Step the clock forward by `dt` — drives animation progression
    /// frame-by-frame in a deterministic test.
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
        // Reading never moves it — every frame samples the same phase.
        assert_eq!(c.now(), Duration::from_millis(500));
        assert_eq!(c.now(), Duration::from_millis(500));
        c.advance(Duration::from_millis(250));
        assert_eq!(c.now(), Duration::from_millis(750));
        // A fixed clock never occludes and schedules no real wait.
        c.skip(Duration::from_secs(10));
        assert_eq!(c.now(), Duration::from_millis(750));
        assert_eq!(c.deadline(Duration::from_secs(1)), None);
    }

    #[test]
    fn realtime_clock_deadline_and_skip_are_exact() {
        let mut c = RealtimeClock::new();
        // Monotonic: two reads never go backward.
        let t0 = c.now();
        assert!(c.now() >= t0);
        // `deadline(at) = origin + at` — a later `at` lands exactly `at` later.
        let d0 = c.deadline(Duration::ZERO).unwrap();
        let d1 = c.deadline(Duration::from_secs(1)).unwrap();
        assert_eq!(d1 - d0, Duration::from_secs(1));
        // `skip` shifts the origin forward, so every future deadline moves by
        // exactly the skipped amount (pure origin arithmetic — no wall clock).
        c.skip(Duration::from_millis(500));
        let d0_after = c.deadline(Duration::ZERO).unwrap();
        assert_eq!(d0_after - d0, Duration::from_millis(500));
    }
}
