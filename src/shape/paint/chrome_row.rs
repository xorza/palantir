//! A node's chrome, as its paint row.

use crate::common::content_hash::ContentHash;
use crate::primitives::geometry::corners::Corners;
use crate::shape::paint::lowered_shadow::LoweredShadow;
use crate::shape::paint::shape_brush::ShapeBrush;
use crate::shape::paint::shape_stroke::ShapeStroke;

#[derive(Clone, Copy, Debug)]
pub(crate) struct ChromeRow {
    pub(crate) fill: ShapeBrush,
    pub(crate) border: ShapeStroke,
    pub(crate) corners: Corners,
    pub(crate) shadow: LoweredShadow,
    pub(crate) hash: ContentHash,
    /// Whether the focus ring draws over this chrome, along the same edge
    /// and corners: set on the one node per tree that holds focus that
    /// came from the keyboard. A flag rather than the stroke, which the
    /// tree holds once as [`Tree::focus_ring`](crate::scene::tree::Tree),
    /// so the ring costs every other chrome row nothing; it is folded
    /// into `hash`, so damage sees it come and go. Kept apart from
    /// `border` because a border widens the padding and a ring must not
    /// move the layout.
    pub(crate) ring: bool,
}

impl ChromeRow {
    /// Whether this row draws no pixel — a row kept only so a
    /// `ClipMode::Rounded` mask can read its corners. Lowering turns a
    /// no-op fill into a transparent solid, so a gradient never needs its
    /// stops read here.
    pub(crate) const fn is_invisible(&self) -> bool {
        matches!(self.fill, ShapeBrush::Solid(color) if color.is_noop())
            && self.border.is_noop()
            && self.shadow.is_noop()
            && !self.ring
    }
}
