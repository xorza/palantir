//! [`Visibility`]: whether a window is hidden, and since when.

use std::time::{Duration, Instant};

/// Whether a window is hidden, by occlusion or by a minimize, and since when: the frame clock
/// skips the span when the window shows again, so resuming emits no giant animation `dt`.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Visibility {
    /// When the window became hidden; `None` while it shows.
    hidden_since: Option<Instant>,
    /// The platform reported the window occluded.
    occluded: bool,
    /// Resized to zero: how Windows reports a minimize (no `Occluded`).
    minimized: bool,
}

impl Visibility {
    pub(super) const fn is_hidden(&self) -> bool {
        self.hidden_since.is_some()
    }

    pub(super) const fn is_minimized(&self) -> bool {
        self.minimized
    }

    /// Set the occlusion at `now`; returns the hidden span this ends, if the window shows again.
    pub(super) fn set_occluded(&mut self, occluded: bool, now: Instant) -> Option<Duration> {
        self.occluded = occluded;
        self.update(now)
    }

    /// Set the minimize at `now`; returns the hidden span this ends, if the window shows again.
    pub(super) fn set_minimized(&mut self, minimized: bool, now: Instant) -> Option<Duration> {
        self.minimized = minimized;
        self.update(now)
    }

    /// Start the hidden span again at `now`, after a frame that ran while hidden: the clock must
    /// skip only the time after it, or a frame stamp would go backwards.
    pub(super) const fn restart(&mut self, now: Instant) {
        if self.hidden_since.is_some() {
            self.hidden_since = Some(now);
        }
    }

    fn update(&mut self, now: Instant) -> Option<Duration> {
        let hidden = self.occluded || self.minimized;
        match (hidden, self.hidden_since) {
            (true, None) => {
                self.hidden_since = Some(now);
                None
            }
            (false, Some(since)) => {
                self.hidden_since = None;
                Some(now.saturating_duration_since(since))
            }
            _ => None,
        }
    }
}
