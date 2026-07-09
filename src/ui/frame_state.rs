//! Submission status of the most recently produced frame. Written by
//! `Ui::frame` (→ pending at frame top) and `WindowRenderer::render`
//! (→ submitted after a successful submit / backbuffer copy). Read by
//! `Ui::classify_frame` to decide whether to rewind the
//! damage snapshot.

/// `Default` (`false`) covers the before-first-frame case: no prior
/// frame to trust, so it reads as unsubmitted and the first
/// `classify_frame` rewinds prev.
#[derive(Debug, Default)]
pub(crate) struct FrameState {
    submitted: bool,
}

impl FrameState {
    pub(crate) fn mark_pending(&mut self) {
        self.submitted = false;
    }
    pub(crate) fn mark_submitted(&mut self) {
        self.submitted = true;
    }
    pub(crate) fn was_last_submitted(&self) -> bool {
        self.submitted
    }
}
