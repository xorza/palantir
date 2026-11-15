//! [`LoweredPaint`]: a box's paint as its record keeps it.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::shape::geometry::closed_geometry::ClosedGeometry;
use crate::shape::paint::lowered_border::LoweredBorder;
use crate::shape::paint::lowered_brush::LoweredBrush;
use crate::shape::paint::lowered_material::LoweredMaterial;
use crate::shape::paint::lowered_shadow::LoweredShadow;

/// A [`Paint`](crate::Paint) lowered for a box, a node's, a rect's or a circle's: its fill
/// interned, its border at its inset, its shadow, and a shape's material.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LoweredPaint {
    pub(crate) fill: LoweredBrush,
    pub(crate) border: LoweredBorder,
    pub(crate) shadow: LoweredShadow,
    pub(crate) material: Option<LoweredMaterial>,
}

impl LoweredPaint {
    /// The owner-local rect this paints over `geometry` in an owner of `owner_size`: the shape's
    /// covering rect, grown by its shadow.
    pub(crate) fn bbox_local(&self, geometry: ClosedGeometry, owner_size: Size) -> Rect {
        let rect = geometry.local_bounds(owner_size);
        if self.shadow.is_noop() {
            rect
        } else {
            rect.union(self.shadow.paint_rect(rect))
        }
    }
}
