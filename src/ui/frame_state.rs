//! Submission status of the most recently produced `FrameReport`.
//! Shared between `Ui` (writes `Pending` at the top of `Ui::frame`,
//! reads `was_last_submitted` to decide whether to invalidate the
//! prev-frame damage snapshot) and the renderer (`Host::render`
//! writes `Submitted` after a successful submit / backbuffer copy).
//! `FrameReport` carries a clone so tests driving `Ui::frame`
//! directly can ack the frame themselves.
//!
//! `AtomicU8` is overkill for the single-threaded renderer path, but
//! cheap and lets `Ui` / `FrameReport` stay `Send`/`Sync` compatible
//! without further constraints.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Debug, Default)]
pub(crate) struct FrameState(Arc<AtomicU8>);

// FrameState::default() leaves the inner byte at 0, which doesn't
// match SUBMITTED below — so the first `was_last_submitted` returns
// false and the first `Ui::should_invalidate_prev` rewinds, exactly
// as wanted.
const FRAME_STATE_PENDING: u8 = 1;
const FRAME_STATE_SUBMITTED: u8 = 2;

impl FrameState {
    pub(crate) fn mark_pending(&self) {
        self.0.store(FRAME_STATE_PENDING, Ordering::Relaxed);
    }
    pub(crate) fn mark_submitted(&self) {
        self.0.store(FRAME_STATE_SUBMITTED, Ordering::Relaxed);
    }
    pub(crate) fn was_last_submitted(&self) -> bool {
        self.0.load(Ordering::Relaxed) == FRAME_STATE_SUBMITTED
    }
}
