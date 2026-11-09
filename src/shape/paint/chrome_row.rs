//! A node's chrome, as its paint row.

use crate::common::content_hash::ContentHash;
use crate::primitives::geometry::corners::Corners;
use crate::shape::paint::lowered_border::LoweredBorder;
use crate::shape::paint::lowered_brush::LoweredBrush;
use crate::shape::paint::lowered_material::LoweredMaterial;
use crate::shape::paint::lowered_shadow::LoweredShadow;

#[derive(Clone, Copy, Debug)]
pub(crate) struct ChromeRow {
    pub(crate) fill: LoweredBrush,
    /// Width is [`Paint::border_inset`](crate::Paint::border_inset)'s fold even when the color paints nothing; the payload normalizes it.
    pub(crate) border: LoweredBorder,
    pub(crate) corners: Corners,
    pub(crate) shadow: LoweredShadow,
    /// The chrome's material; one that captures makes the chrome a backdrop.
    pub(crate) material: Option<LoweredMaterial>,
    pub(crate) hash: ContentHash,
    /// Whether the focus ring draws over this chrome: set on the one node per tree holding keyboard focus. A flag, not a stroke ([`Tree::focus_ring`](crate::scene::tree::Tree) holds it once); folded into `hash` for damage.
    pub(crate) ring: bool,
}

impl ChromeRow {
    /// Whether this row draws no pixel (kept only for a `ClipMode::Rounded` mask's corners).
    pub(crate) const fn is_invisible(&self) -> bool {
        matches!(self.fill, LoweredBrush::Solid(color) if color.is_noop())
            && self.border.is_noop()
            && self.shadow.is_noop()
            && self.material.is_none()
            && !self.ring
    }
}
