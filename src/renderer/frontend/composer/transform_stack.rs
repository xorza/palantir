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
    cur: TranslateScale,
}

impl TransformStack {
    /// Open a pass at the identity, keeping the stack's capacity.
    pub(super) fn clear(&mut self) {
        self.saved.clear();
        self.cur = TranslateScale::IDENTITY;
    }

    /// The live product.
    pub(super) const fn cur(&self) -> TranslateScale {
        self.cur
    }

    pub(super) const fn scale(&self) -> f32 {
        self.cur.scale
    }

    pub(super) const fn apply_rect(&self, rect: Rect) -> Rect {
        self.cur.apply_rect(rect)
    }

    pub(super) fn push(&mut self, t: TranslateScale) {
        self.saved.push(self.cur);
        self.cur = self.cur.compose(t);
    }

    /// Panics on a `PopTransform` with no matching push (a malformed paint stream).
    pub(super) fn pop(&mut self) {
        self.cur = self
            .saved
            .pop()
            .expect("PopTransform without matching PushTransform");
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::internals::scratch_report;
    use crate::renderer::frontend::composer::transform_stack::TransformStack;

    impl TransformStack {
        pub(crate) fn scratch_bytes(&self) -> usize {
            let Self { saved, cur: _ } = self;
            scratch_report::vec_bytes(saved)
        }
    }
}
