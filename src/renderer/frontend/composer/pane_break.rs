//! Where the main pass last breaks for a pane, and what it paints after.

use crate::primitives::geometry::urect::URect;

/// The barrier of the last pane that broke the main pass, `0` before any, and the bounds of every
/// draw since. A pane whose footprint misses those bounds sees at that break what it would see at
/// its own place, so it captures there and breaks nothing.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct PaneBreak {
    barrier: u32,
    since: URect,
}

impl PaneBreak {
    /// Count a draw at `bounds` as painted after the break.
    pub(super) const fn paint(&mut self, bounds: URect) {
        self.since = self.since.union(bounds);
    }

    /// The break a pane whose footprint is `footprint` captures at, if what was painted since
    /// misses it.
    pub(super) const fn shared(&self, footprint: URect) -> Option<u32> {
        if self.since.intersects(footprint) {
            None
        } else {
            Some(self.barrier)
        }
    }

    /// Break at `barrier`, with nothing painted since.
    pub(super) const fn open(&mut self, barrier: u32) {
        *self = Self {
            barrier,
            since: URect::ZERO,
        };
    }
}
