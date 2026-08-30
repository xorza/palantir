//! How a layer root is measured and where it lands afterwards.

use crate::layout::types::overlay::OverlayPosition;
use crate::primitives::approx::FloatHash;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use glam::Vec2;

/// Where a layer root's origin comes from.
///
/// The two forms are exclusive — an origin is either known before
/// measure or derived from it — which is the whole of what they decide.
/// The size cap beside them on [`Placement`] is orthogonal, so setting
/// one never discards the other.
#[derive(Clone, Copy, Debug)]
pub(crate) enum PlacementOrigin {
    /// Top-left fixed at this surface-space point.
    Anchor(Vec2),
    /// Resolved from the measured size against a screen-space anchor
    /// rect — the flip-or-shift-to-fit form popups, menus and tooltips
    /// want.
    Overlay(OverlayPosition),
}

/// Measurement and post-measure placement policy for one layer root.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Placement {
    pub(crate) origin: PlacementOrigin,
    /// Upper bound on the available extent, clamped to the surface at
    /// [`Self::available`]. `None` leaves an anchored root the space
    /// from its anchor to the surface edge, and an overlay the whole
    /// surface — the extent each resolves its origin against.
    pub(crate) max_size: Option<Size>,
}

impl Placement {
    /// Feed this placement to a hasher under visual canonicalization.
    ///
    /// Placement lives outside the node hashes but changes arranged
    /// rects, so the cascade fingerprint folds it in — here rather than
    /// there, because what the fields carry is this type's own
    /// business. Inherent rather than an [`FloatHash`] impl, on the same
    /// terms as [`OverlayPosition::hash_visual`]: the trait's other half
    /// is the `Hash`/`PartialEq` agreement, which this type does not have.
    pub(crate) fn hash_visual<H: std::hash::Hasher>(&self, state: &mut H) {
        match self.origin {
            PlacementOrigin::Anchor(anchor) => {
                state.write_u8(0);
                anchor.hash_visual(state);
            }
            PlacementOrigin::Overlay(position) => {
                state.write_u8(1);
                position.hash_visual(state);
            }
        }
        match self.max_size {
            Some(size) => {
                state.write_u8(1);
                size.hash_visual(state);
            }
            None => state.write_u8(0),
        }
    }

    pub(crate) const fn fixed(anchor: Vec2, max_size: Option<Size>) -> Self {
        Self {
            origin: PlacementOrigin::Anchor(anchor),
            max_size,
        }
    }

    /// Replace the origin, keeping any size cap.
    pub(crate) const fn with_anchor(self, anchor: Vec2) -> Self {
        Self {
            origin: PlacementOrigin::Anchor(anchor),
            ..self
        }
    }

    /// Replace the size cap, keeping the origin.
    pub(crate) const fn with_size(self, max_size: Size) -> Self {
        Self {
            max_size: Some(max_size),
            ..self
        }
    }

    pub(crate) fn available(self, surface: Rect) -> Size {
        match (self.max_size, self.origin) {
            (Some(size), _) => Size::new(size.w.min(surface.size.w), size.h.min(surface.size.h)),
            (None, PlacementOrigin::Anchor(anchor)) => {
                let remaining = (surface.max() - anchor).max(Vec2::ZERO);
                Size::new(remaining.x, remaining.y)
            }
            (None, PlacementOrigin::Overlay(_)) => surface.size,
        }
    }

    pub(crate) fn origin(self, measured: Size, surface: Rect) -> Vec2 {
        match self.origin {
            PlacementOrigin::Anchor(anchor) => anchor,
            PlacementOrigin::Overlay(position) => position.resolve(measured, surface),
        }
    }
}

/// An anchor-relative position *is* a placement, which is what lets
/// [`LayerScope::placement`](crate::ui::layer_scope::LayerScope::placement)
/// take either form through one `impl Into<Placement>` parameter.
impl From<OverlayPosition> for Placement {
    fn from(position: OverlayPosition) -> Self {
        Self {
            origin: PlacementOrigin::Overlay(position),
            max_size: None,
        }
    }
}

impl Default for Placement {
    fn default() -> Self {
        Self::fixed(Vec2::ZERO, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::types::overlay::OverlayPosition;

    const SURFACE: Rect = Rect::new(0.0, 0.0, 200.0, 100.0);
    const MEASURED: Size = Size::new(50.0, 30.0);

    /// A gap of 4 below a 20x6 anchor at (40, 10) puts the body's top at
    /// `10 + 6 + 4 = 20`, and `AxisAlign::Start` puts its left at the
    /// anchor's 40. Both fit, so neither the flip nor the clamp fires.
    fn overlay() -> Placement {
        OverlayPosition::below(Rect::new(40.0, 10.0, 20.0, 6.0), 4.0).into()
    }

    #[test]
    fn a_size_cap_bounds_an_overlay_without_moving_its_origin() {
        let capped = overlay().with_size(Size::new(80.0, 40.0));
        assert_eq!(capped.available(SURFACE), Size::new(80.0, 40.0));
        assert_eq!(overlay().available(SURFACE), SURFACE.size);
        assert_eq!(
            capped.origin(MEASURED, SURFACE),
            Vec2::new(40.0, 20.0),
            "the cap bounds the measure, the overlay still resolves the origin",
        );
    }

    #[test]
    fn an_anchor_and_a_size_cap_land_the_same_way_in_either_order() {
        let anchor = Vec2::new(12.0, 7.0);
        let cap = Size::new(80.0, 40.0);
        let anchor_first = overlay().with_anchor(anchor).with_size(cap);
        let size_first = overlay().with_size(cap).with_anchor(anchor);
        for placed in [anchor_first, size_first] {
            assert_eq!(placed.available(SURFACE), cap);
            assert_eq!(placed.origin(MEASURED, SURFACE), anchor);
        }
    }

    /// Without a cap an anchored root gets the space from its anchor to
    /// the surface edge — `200 - 12` by `100 - 7`.
    #[test]
    fn an_uncapped_anchor_measures_against_the_rest_of_the_surface() {
        let placed = Placement::default().with_anchor(Vec2::new(12.0, 7.0));
        assert_eq!(placed.available(SURFACE), Size::new(188.0, 93.0));
    }
}
