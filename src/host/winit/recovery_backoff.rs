//! [`RecoveryBackoff`]: when a host tries again to attach a device after a recovery failed.

use std::time::{Duration, Instant};

/// First wait after a failed attempt: a reset still running refuses an adapter for a while.
const RETRY_MIN: Duration = Duration::from_millis(100);

/// Ceiling for the doubling wait, so a GPU back from a reset is picked up within it.
const RETRY_MAX: Duration = Duration::from_secs(2);

/// How long attempts that fail with an error run before the host gives up. Windows resets a GPU
/// job that runs past `TdrDelay` (2 s) and gives the driver `TdrDdiDelay` (5 s) to leave it
/// (<https://learn.microsoft.com/windows-hardware/drivers/display/tdr-registry-keys>); the window
/// sits far above one reset, so a storm of them does not end the app.
const GIVE_UP_AFTER: Duration = Duration::from_secs(30);

/// The failed attempts of the recovery under way, if one failed; healthy while `None`.
#[derive(Debug, Default)]
pub(super) struct RecoveryBackoff {
    failing: Option<Failing>,
}

#[derive(Clone, Copy, Debug)]
struct Failing {
    since: Instant,
    delay: Duration,
    next: Instant,
}

impl RecoveryBackoff {
    /// How long until the next attempt may run, or `None` when one may run at `now`.
    pub(super) fn wait_left(&self, now: Instant) -> Option<Duration> {
        let next = self.failing?.next;
        (now < next).then(|| next - now)
    }

    /// Note a failed attempt at `now`. The next waits [`RETRY_MIN`], doubling to [`RETRY_MAX`].
    pub(super) fn failed(&mut self, now: Instant) {
        let (since, delay) = match self.failing {
            None => (now, RETRY_MIN),
            Some(failing) => (failing.since, (failing.delay * 2).min(RETRY_MAX)),
        };
        self.failing = Some(Failing {
            since,
            delay,
            next: now + delay,
        });
    }

    /// Whether attempts have failed for [`GIVE_UP_AFTER`] since the first, so the next error ends
    /// the host. A device lost again as it was attached never ends it: the UI state stays, and
    /// there is no error to report.
    pub(super) fn exhausted(&self, now: Instant) -> bool {
        self.failing
            .is_some_and(|failing| now.duration_since(failing.since) >= GIVE_UP_AFTER)
    }

    /// The device attached and held: the next loss starts again at [`RETRY_MIN`].
    pub(super) const fn succeeded(&mut self) {
        self.failing = None;
    }
}

#[cfg(test)]
mod tests {
    use crate::host::winit::recovery_backoff::{GIVE_UP_AFTER, RETRY_MIN, RecoveryBackoff};
    use std::time::{Duration, Instant};

    /// The wait doubles from 100 ms and holds at 2 s: 100, 200, 400, 800, 1600, 2000, 2000 ms.
    /// An attempt waits it out; the host gives up only once 30 s passed since the first failure,
    /// and a success starts over.
    #[test]
    fn a_failing_recovery_waits_longer_each_time_and_gives_up_after_its_window() {
        let t0 = Instant::now();
        let mut backoff = RecoveryBackoff::default();
        assert_eq!(backoff.wait_left(t0), None);
        assert!(!backoff.exhausted(t0));

        let mut now = t0;
        let mut waits = Vec::new();
        for _ in 0..7 {
            backoff.failed(now);
            let wait = backoff.wait_left(now).expect("a failed attempt waits");
            assert_eq!(backoff.wait_left(now + wait / 2), Some(wait / 2));
            assert_eq!(backoff.wait_left(now + wait), None);
            waits.push(wait.as_millis());
            now += wait;
        }
        assert_eq!(waits, [100, 200, 400, 800, 1600, 2000, 2000]);

        assert!(!backoff.exhausted(t0 + GIVE_UP_AFTER - Duration::from_millis(1)));
        assert!(backoff.exhausted(t0 + GIVE_UP_AFTER));

        backoff.succeeded();
        assert!(!backoff.exhausted(t0 + GIVE_UP_AFTER));
        backoff.failed(now);
        assert_eq!(backoff.wait_left(now), Some(RETRY_MIN));
    }
}
