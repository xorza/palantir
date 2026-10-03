//! A node's chrome, as its paint row.

use crate::common::content_hash::ContentHash;
use crate::primitives::corners::Corners;
use crate::scene::shapes::paint::lowered_shadow::LoweredShadow;
use crate::scene::shapes::paint::shape_brush::ShapeBrush;
use crate::scene::shapes::paint::shape_stroke::ShapeStroke;

#[derive(Clone, Copy, Debug)]
pub(crate) struct ChromeRow {
    pub(crate) fill: ShapeBrush,
    pub(crate) border: ShapeStroke,
    pub(crate) corners: Corners,
    pub(crate) shadow: LoweredShadow,
    pub(crate) hash: ContentHash,
}

impl ChromeRow {
    /// Whether this row draws no pixel — a row kept only so a
    /// `ClipMode::Rounded` mask can read its corners. Lowering turns a
    /// no-op fill into a transparent solid, so a gradient never needs its
    /// stops read here.
    pub(crate) const fn paints_nothing(&self) -> bool {
        matches!(self.fill, ShapeBrush::Solid(color) if color.is_noop())
            && self.border.is_noop()
            && self.shadow.is_noop()
    }
}
