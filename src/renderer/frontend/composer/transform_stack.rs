//! The transform in force during a compose pass, and the stack it comes
//! off.

use crate::primitives::rect::Rect;
use crate::primitives::translate_scale::TranslateScale;
use glam::Vec2;

/// The walk transform: the live product every draw is placed by, plus the
/// ancestors a `PopTransform` restores it from.
///
/// **One type, because they are one stack.** The product is what every
/// handler reads and the saved ancestors are what a pop returns it to;
/// holding the two apart made a pop a two-place operation and put the
/// live value on the per-frame session while its own history stayed on
/// the retained scratch. The `Vec` is the only allocation, and it is kept
/// across frames for its capacity; [`Self::reset`] opens each pass.
#[derive(Debug, Default)]
pub(super) struct TransformStack {
    /// Ancestor products, innermost last — what each pop restores.
    saved: Vec<TranslateScale>,
    current: TranslateScale,
}

impl TransformStack {
    /// Open a pass at the identity, keeping the stack's capacity.
    pub(super) fn clear(&mut self) {
        self.saved.clear();
        self.current = TranslateScale::IDENTITY;
    }

    /// The live product — for a handler that needs the whole value, or
    /// its scale.
    pub(super) fn current(&self) -> TranslateScale {
        self.current
    }

    pub(super) fn scale(&self) -> f32 {
        self.current.scale
    }

    pub(super) fn apply_rect(&self, rect: Rect) -> Rect {
        self.current.apply_rect(rect)
    }

    pub(super) fn apply_point(&self, point: Vec2) -> Vec2 {
        self.current.apply_point(point)
    }

    pub(super) fn push(&mut self, t: TranslateScale) {
        self.saved.push(self.current);
        self.current = self.current.compose(t);
    }

    /// Panics on a `PopTransform` with no matching push — a malformed
    /// paint stream, like its clip counterpart.
    pub(super) fn pop(&mut self) {
        self.current = self
            .saved
            .pop()
            .expect("PopTransform without matching PushTransform");
    }
}
