//! The transform in force during a compose pass, and its stack.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::translate_scale::TranslateScale;

/// The walk transform: the live product every draw is placed by, plus the ancestors a `PopTransform` restores.
///
/// The `Vec` is kept across frames for capacity; [`Self::clear`] opens each pass.
#[derive(Debug, Default)]
pub(super) struct TransformStack {
    /// Ancestor products, innermost last.
    saved: Vec<TranslateScale>,
    current: TranslateScale,
}

impl TransformStack {
    /// Open a pass at the identity, keeping the stack's capacity.
    pub(super) fn clear(&mut self) {
        self.saved.clear();
        self.current = TranslateScale::IDENTITY;
    }

    /// The live product.
    pub(super) const fn current(&self) -> TranslateScale {
        self.current
    }

    pub(super) const fn scale(&self) -> f32 {
        self.current.scale
    }

    pub(super) const fn apply_rect(&self, rect: Rect) -> Rect {
        self.current.apply_rect(rect)
    }

    pub(super) fn push(&mut self, t: TranslateScale) {
        self.saved.push(self.current);
        self.current = self.current.compose(t);
    }

    /// Panics on a `PopTransform` with no matching push (a malformed paint stream).
    pub(super) fn pop(&mut self) {
        self.current = self
            .saved
            .pop()
            .expect("PopTransform without matching PushTransform");
    }
}
