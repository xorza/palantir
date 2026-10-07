//! A node's chrome, as its paint row.

use crate::common::content_hash::ContentHash;
use crate::primitives::geometry::corners::Corners;
use crate::shape::paint::lowered_shadow::LoweredShadow;
use crate::shape::paint::shape_brush::ShapeBrush;
use crate::shape::paint::shape_stroke::ShapeStroke;

#[derive(Clone, Copy, Debug)]
pub(crate) struct ChromeRow {
    pub(crate) fill: ShapeBrush,
    /// Width is [`Background::border_inset`](crate::Background)'s fold even when the colour paints nothing; the payload normalizes it.
    pub(crate) border: ShapeStroke,
    pub(crate) corners: Corners,
    pub(crate) shadow: LoweredShadow,
    pub(crate) hash: ContentHash,
    /// Whether the focus ring draws over this chrome: set on the one node per tree holding keyboard focus. A flag, not a stroke ([`Tree::focus_ring`](crate::scene::tree::Tree) holds it once); folded into `hash` for damage.
    pub(crate) ring: bool,
}

impl ChromeRow {
    /// Whether this row draws no pixel (kept only for a `ClipMode::Rounded` mask's corners).
    pub(crate) const fn is_invisible(&self) -> bool {
        matches!(self.fill, ShapeBrush::Solid(color) if color.is_noop())
            && self.border.is_noop()
            && self.shadow.is_noop()
            && !self.ring
    }
}
