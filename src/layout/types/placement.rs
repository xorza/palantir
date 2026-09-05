//! How a layer root is measured and where it lands afterwards.

use crate::layout::types::anchor::Anchor;
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
pub(crate) enum Origin {
    /// Top-left fixed at this surface-space point.
    Fixed(Vec2),
    /// Resolved from the measured size against a screen-space anchor
    /// rect — the flip-or-shift-to-fit form popups, menus and tooltips
    /// want.
    Anchored(Anchor),
}

/// Measurement and post-measure placement policy for one layer root.
///
/// The *storage* form, not the authoring one: it hangs off the root slot,
/// the layout engine reads [`Self::available`] and [`Self::origin`] off it
/// two passes after the record that set it, and the measure cache folds it
/// into a fingerprint. [`LayerScope`](crate::LayerScope) is its public
/// face — `fixed_at` and `anchored` write [`Self::origin`], `max_size`
/// writes the other field — which is why nothing publishes this type as
/// a value. [`Anchor`] is one of the two origin rules it holds, and the
/// only one with enough parameters to need a name of its own.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Placement {
    pub(crate) origin: Origin,
    /// Upper bound on the available extent, clamped to the surface at
    /// [`Self::available`]. `None` leaves a [`Origin::Fixed`] root the
    /// space from its point to the surface edge, and an
    /// [`Origin::Anchored`] one the whole surface — the extent each
    /// resolves its origin against.
    pub(crate) max_size: Option<Size>,
}

impl Placement {
    /// Feed this placement to a hasher under visual canonicalization.
    ///
    /// Placement lives outside the node hashes but changes arranged
    /// rects, so the cascade fingerprint folds it in — here rather than
    /// there, because what the fields carry is this type's own
    /// business. Inherent rather than an [`FloatHash`] impl, on the same
    /// terms as [`Anchor::hash_visual`]: the trait's other half
    /// is the `Hash`/`PartialEq` agreement, which this type does not have.
    pub(crate) fn hash_visual<H: std::hash::Hasher>(&self, state: &mut H) {
        match self.origin {
            Origin::Fixed(point) => {
                state.write_u8(0);
                point.hash_visual(state);
            }
            Origin::Anchored(anchor) => {
                state.write_u8(1);
                anchor.hash_visual(state);
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

    /// Replace the origin with a fixed point, keeping any size cap.
    pub(crate) const fn with_fixed(self, point: Vec2) -> Self {
        Self {
            origin: Origin::Fixed(point),
            ..self
        }
    }

    /// Replace the origin with one resolved after measure, keeping any
    /// size cap.
    pub(crate) const fn with_anchored(self, anchor: Anchor) -> Self {
        Self {
            origin: Origin::Anchored(anchor),
            ..self
        }
    }

    /// Replace the size cap, keeping the origin.
    pub(crate) const fn with_max_size(self, max_size: Size) -> Self {
        Self {
            max_size: Some(max_size),
            ..self
        }
    }

    pub(crate) fn available(self, surface: Rect) -> Size {
        match (self.max_size, self.origin) {
            (Some(size), _) => Size::new(size.w.min(surface.size.w), size.h.min(surface.size.h)),
            (None, Origin::Fixed(anchor)) => {
                let remaining = (surface.max() - anchor).max(Vec2::ZERO);
                Size::new(remaining.x, remaining.y)
            }
            (None, Origin::Anchored(_)) => surface.size,
        }
    }

    pub(crate) fn origin(self, measured: Size, surface: Rect) -> Vec2 {
        match self.origin {
            Origin::Fixed(anchor) => anchor,
            Origin::Anchored(position) => position.resolve(measured, surface),
        }
    }
}

/// The surface origin with the whole surface available — what a layer
/// that sets no placement gets, and what a full-surface overlay wants.
impl Default for Placement {
    fn default() -> Self {
        Self {
            origin: Origin::Fixed(Vec2::ZERO),
            max_size: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::types::anchor::Anchor;

    const SURFACE: Rect = Rect::new(0.0, 0.0, 200.0, 100.0);
    const MEASURED: Size = Size::new(50.0, 30.0);

    /// A gap of 4 below a 20x6 rect at (40, 10) puts the body's top at
    /// `10 + 6 + 4 = 20`, and `AxisAlign::Start` puts its left at the
    /// rect's 40. Both fit, so neither the flip nor the clamp fires.
    fn anchored() -> Placement {
        Placement::default().with_anchored(Anchor::below(Rect::new(40.0, 10.0, 20.0, 6.0)).gap(4.0))
    }

    #[test]
    fn a_size_cap_bounds_an_anchored_root_without_moving_its_origin() {
        let capped = anchored().with_max_size(Size::new(80.0, 40.0));
        assert_eq!(capped.available(SURFACE), Size::new(80.0, 40.0));
        assert_eq!(anchored().available(SURFACE), SURFACE.size);
        assert_eq!(
            capped.origin(MEASURED, SURFACE),
            Vec2::new(40.0, 20.0),
            "the cap bounds the measure, the anchor still resolves the origin",
        );
    }

    #[test]
    fn a_fixed_point_and_a_size_cap_land_the_same_way_in_either_order() {
        let point = Vec2::new(12.0, 7.0);
        let cap = Size::new(80.0, 40.0);
        let point_first = anchored().with_fixed(point).with_max_size(cap);
        let size_first = anchored().with_max_size(cap).with_fixed(point);
        for placed in [point_first, size_first] {
            assert_eq!(placed.available(SURFACE), cap);
            assert_eq!(placed.origin(MEASURED, SURFACE), point);
        }
    }

    /// Without a cap a fixed root gets the space from its point to the
    /// surface edge — `200 - 12` by `100 - 7`.
    #[test]
    fn an_uncapped_fixed_root_measures_against_the_rest_of_the_surface() {
        let placed = Placement::default().with_fixed(Vec2::new(12.0, 7.0));
        assert_eq!(placed.available(SURFACE), Size::new(188.0, 93.0));
    }
}
