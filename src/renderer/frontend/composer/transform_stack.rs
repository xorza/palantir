//! The transform in force during a compose pass, and its stack.

use crate::primitives::geometry::rect::{EdgeRounding, Rect};
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::renderer::frontend::composer::physical_map::PhysicalMap;
use glam::Vec2;

/// The walk transform: the live product every draw is placed by, plus the ancestors a `PopTransform` restores.
///
/// The `Vec` is kept across frames for capacity; [`Self::clear`] opens each pass.
#[derive(Debug, Default)]
pub(super) struct TransformStack {
    /// Ancestor products, innermost last.
    saved: Vec<TranslateScale>,
    cur: TranslateScale,
    /// The display's logical-to-physical factor, fixed for a pass.
    display_scale: f32,
    /// `cur` between physical px: its translation times `display_scale`, its scale as is. Kept in
    /// step with `cur` so a draw reads it instead of rebuilding it.
    physical: TranslateScale,
}

impl TransformStack {
    /// Open a pass at the identity under `display_scale`, keeping the stack's capacity.
    pub(super) fn clear(&mut self, display_scale: f32) {
        self.saved.clear();
        self.display_scale = display_scale;
        self.set(TranslateScale::IDENTITY);
    }

    const fn set(&mut self, cur: TranslateScale) {
        self.cur = cur;
        self.physical = cur.in_units_of(self.display_scale);
    }

    /// Physical px per owner-local unit under the live product.
    pub(super) const fn physical_scale(&self) -> f32 {
        self.physical.scale() * self.display_scale
    }

    /// `rect`, in logical px under the live product, in physical px: scaled by the display and
    /// landed per `edges` in its own frame, then placed by the live product exactly. Blink snaps
    /// layout to pixels and applies the transform to the drawing context after, so a translation
    /// moves snapped geometry by sub-pixel steps and lands it back on its pixels at the identity.
    /// Refused: snapping after the transform, which moved every animated translation by whole
    /// pixels and held slow motion still for frames.
    pub(super) fn physical_rect(&self, rect: Rect, edges: EdgeRounding) -> Rect {
        self.physical
            .apply_rect(rect.scaled_by(self.display_scale, edges))
    }

    /// The map from an owner's local space, at `origin`, to physical px under the live product.
    pub(super) const fn physical_map(&self, origin: Vec2) -> PhysicalMap {
        PhysicalMap::new(self.cur, origin, self.display_scale)
    }

    pub(super) const fn cur(&self) -> TranslateScale {
        self.cur
    }

    pub(super) const fn scale(&self) -> f32 {
        self.cur.scale()
    }

    pub(super) const fn apply_rect(&self, rect: Rect) -> Rect {
        self.cur.apply_rect(rect)
    }

    pub(super) fn push(&mut self, t: TranslateScale) {
        self.saved.push(self.cur);
        self.set(self.cur.compose(t));
    }

    /// Panics on a `PopTransform` with no matching push (a malformed paint stream).
    pub(super) fn pop(&mut self) {
        let restored = self
            .saved
            .pop()
            .expect("PopTransform without matching PushTransform");
        self.set(restored);
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::internals::scratch_report;
    use crate::renderer::frontend::composer::transform_stack::TransformStack;

    impl TransformStack {
        pub(crate) fn scratch_bytes(&self) -> usize {
            let Self { saved, .. } = self;
            scratch_report::vec_bytes(saved)
        }
    }
}
