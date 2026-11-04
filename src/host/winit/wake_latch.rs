//! [`WakeLatch`]: a window's one wake in flight.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether a window's [`Waker`](crate::Waker) has a repaint event in flight: a wake sends one only
/// when none is, and the event loop lowers the latch as it takes the event, so a worker that
/// wakes the window once per item sends one event a frame, not one an item, through the
/// unbounded event-loop queue, as egui's `request_repaint` coalesces at its source.
#[derive(Clone, Debug, Default)]
pub(crate) struct WakeLatch(Arc<AtomicBool>);

impl WakeLatch {
    /// Raise the latch; whether this wake is the one that sends the event.
    pub(crate) fn raise(&self) -> bool {
        !self.0.swap(true, Ordering::AcqRel)
    }

    /// Lower the latch as the loop takes its event, before the frame it asks for, so a wake the
    /// frame does not see sends another. A swap, not a store: a wake whose raise read the latch
    /// still up sent nothing, and only an acquire that reads its release orders what it
    /// published before this frame (*Rust Atomics and Locks*, ch. 3).
    pub(crate) fn lower(&self) {
        let _ = self.0.swap(false, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use crate::host::winit::wake_latch::WakeLatch;

    /// The first wake sends and the rest wait for the event to be taken; once taken, the next
    /// wake sends again. A clone is the same latch, as each wake of one window shares it.
    #[test]
    fn one_wake_sends_until_its_event_is_taken() {
        let latch = WakeLatch::default();
        let other = latch.clone();
        assert!(latch.raise());
        assert!(!latch.raise());
        assert!(!other.raise());
        other.lower();
        assert!(latch.raise());
    }
}
