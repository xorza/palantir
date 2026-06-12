//! Submission status of the most recently produced frame. Written by
//! `Ui::frame` (→ `Pending` at frame top) and `WindowRenderer::render` (→
//! `Submitted` after a successful submit / backbuffer copy). Read by
//! `Ui::classify_frame` to decide whether to rewind the
//! damage snapshot.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum State {
    /// Default. Treated like `Pending` by `was_last_submitted` so the
    /// very first frame rewinds prev (no prior frame to trust).
    #[default]
    Initial,
    Pending,
    Submitted,
}

#[derive(Debug, Default)]
pub(crate) struct FrameState(State);

impl FrameState {
    pub(crate) fn mark_pending(&mut self) {
        self.0 = State::Pending;
    }
    pub(crate) fn mark_submitted(&mut self) {
        self.0 = State::Submitted;
    }
    pub(crate) fn was_last_submitted(&self) -> bool {
        self.0 == State::Submitted
    }
}
