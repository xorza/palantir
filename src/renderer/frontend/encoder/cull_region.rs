//! What a partial frame's encode keeps.

use crate::damage::region::DamageRegion;
use crate::primitives::geometry::rect::Rect;

/// The screen a partial frame encodes: its damage, and the footprints of the panes it repaints,
/// which the main pass repaints when such a pane captures.
#[derive(Clone, Copy, Debug)]
pub(super) struct CullRegion<'a> {
    pub(super) damage: &'a DamageRegion,
    pub(super) footprints: &'a [Rect],
}

impl CullRegion<'_> {
    pub(super) fn any_intersects(self, r: Rect) -> bool {
        self.damage.any_intersects(r) || self.footprints.iter().any(|f| f.intersects(r))
    }
}
