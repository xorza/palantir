//! The clip in force during a compose pass, and the stack it comes off.

use crate::primitives::span::Span;
use crate::primitives::urect::URect;

/// One clip level: the resolved scissor plus the rounded-mask chain that
/// travels with it, so a `PopClip` restores both as a unit.
#[derive(Clone, Copy, Debug)]
pub(super) struct ClipFrame {
    pub(super) scissor: URect,
    /// Outer→inner chain of rounded masks active for this frame's
    /// subtree — a span into `RenderBuffer.rounded_clips`. A rounded
    /// push extends the parent chain with its own mask; a rect push
    /// inherits it verbatim. Empty = no rounded ancestor.
    pub(super) chain: Span,
}

/// The nested clips a compose walk has open, innermost last.
///
/// Compose-time scratch, bounded by tree depth (typically <8) and kept
/// across frames for its capacity. Push and pop are deliberately *not*
/// here: entering a clip closes the group and batch the outgoing one
/// owned, and that is a decision about the output buffer rather than
/// about this stack — see `ComposeSession::push_clip`.
#[derive(Debug, Default)]
pub(super) struct ClipStack {
    frames: Vec<ClipFrame>,
}

impl ClipStack {
    /// The clip in force: the stack top, or none at the root.
    ///
    /// **Derived, not cached.** A mirror of the top would have to be
    /// reassigned in lockstep with every push and pop, and the readers
    /// split across it: the cull test and the clear fold ask one
    /// question, the text path another. The stack is the only owner, so
    /// there is nothing for them to disagree about.
    pub(super) fn top(&self) -> Option<ClipFrame> {
        self.frames.last().copied()
    }

    pub(super) fn scissor(&self) -> Option<URect> {
        self.top().map(|frame| frame.scissor)
    }

    pub(super) fn chain(&self) -> Span {
        self.top().map_or(Span::default(), |frame| frame.chain)
    }

    /// The clip that will be in force once the top is popped.
    pub(super) fn parent(&self) -> Option<ClipFrame> {
        self.frames
            .len()
            .checked_sub(2)
            .map(|below| self.frames[below])
    }

    pub(super) fn push(&mut self, frame: ClipFrame) {
        self.frames.push(frame);
    }

    /// Panics on a `PopClip` with no matching push, which is a malformed
    /// paint stream rather than a state the composer can answer for.
    pub(super) fn pop(&mut self) {
        self.frames
            .pop()
            .expect("PopClip without matching PushClip");
    }

    pub(super) fn clear(&mut self) {
        self.frames.clear();
    }

    /// `true` when `bounds` has no viewport area or falls entirely
    /// outside the clip in force — the caller should skip emission.
    /// Identical reject shape at every shape-draw site; centralising it
    /// keeps each handler from growing its own variant.
    pub(super) fn culls(&self, bounds: URect) -> bool {
        bounds.is_paint_empty() || self.scissor().is_some_and(|s| !bounds.intersects(s))
    }
}
