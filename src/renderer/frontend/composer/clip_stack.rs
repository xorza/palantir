//! The clip in force during a compose pass, and the stack it comes off.

use crate::common::span::Span;
use crate::primitives::geometry::urect::URect;

/// One clip level: the resolved scissor plus the rounded-mask chain that
/// travels with it, so a `PopClip` restores both as a unit.
#[derive(Clone, Copy, Debug)]
pub(super) struct ClipFrame {
    pub(super) scissor: URect,
    /// Outer→inner chain of rounded masks for this frame's subtree, a span into
    /// `RenderBuffer.rounded_clips`. A rounded push extends the parent chain; a rect push inherits it.
    /// Empty = no rounded ancestor.
    pub(super) chain: Span,
}

/// The nested clips a compose walk has open, innermost last.
///
/// Compose-time scratch, bounded by tree depth (typically <8) and kept across frames for its
/// capacity. `push` and `pop` are raw: entering a clip also closes the group and batch the
/// outgoing one owned, a decision about the output buffer made in
/// `ComposeSession::push_clip`.
#[derive(Debug, Default)]
pub(super) struct ClipStack {
    frames: Vec<ClipFrame>,
}

impl ClipStack {
    /// The clip in force: the stack top, or none at the root.
    ///
    /// **Derived, not cached.** A mirror of the top would need reassigning in lockstep with every push
    /// and pop, and readers would split across it. The stack is the only owner, so there is nothing for
    /// them to disagree about.
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

    /// Panics on a `PopClip` with no matching push, a malformed paint stream the composer cannot answer for.
    pub(super) fn pop(&mut self) {
        self.frames
            .pop()
            .expect("PopClip without matching PushClip");
    }

    pub(super) fn clear(&mut self) {
        self.frames.clear();
    }

    /// `bounds` held inside the clip in force: the pixels a draw of that extent can reach, paint-empty
    /// exactly when the draw is culled.
    ///
    /// The same reject shape serves every shape-draw site, and the rect each then registers as occupied:
    /// text's batch scissor is the union of its runs' clipped bounds, and the quad and higher tiers order
    /// against what they paint, not what they would have painted unclipped.
    pub(super) fn clamped(&self, bounds: URect) -> URect {
        match self.scissor() {
            Some(scissor) => bounds.clamp_to(scissor),
            None => bounds,
        }
    }
}
