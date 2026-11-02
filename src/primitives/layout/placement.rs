//! How a layer root is measured and where it lands afterwards.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::anchor::Anchor;
use glam::Vec2;

/// Where a layer root's origin comes from: known before measure, or derived from it.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Origin {
    Fixed(Vec2),
    Anchored(Anchor),
}

/// Measurement and post-measure placement policy for one layer root: the storage form of what
/// [`LayerScope`](crate::LayerScope) authors; the measure cache keys the root on its available size.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Placement {
    pub(crate) origin: Origin,
    /// Upper bound on the available extent, never past the room the origin leaves: the space from a
    /// fixed point to the surface edge, or the whole surface for an anchored root. `None` leaves
    /// that room.
    pub(crate) max_size: Option<Size>,
}

impl Placement {
    pub(crate) const fn with_fixed(self, point: Vec2) -> Self {
        Self {
            origin: Origin::Fixed(point),
            ..self
        }
    }

    pub(crate) const fn with_anchored(self, anchor: Anchor) -> Self {
        Self {
            origin: Origin::Anchored(anchor),
            ..self
        }
    }

    pub(crate) const fn with_max_size(self, max_size: Size) -> Self {
        Self {
            max_size: Some(max_size),
            ..self
        }
    }

    pub(crate) fn available(self, surface: Rect) -> Size {
        let room = match self.origin {
            Origin::Fixed(point) => surface.size.room_past(point - surface.min),
            Origin::Anchored(_) => surface.size,
        };
        self.max_size.map_or(room, |cap| cap.min(room))
    }

    pub(crate) fn origin(self, measured: Size, surface: Rect) -> Vec2 {
        match self.origin {
            Origin::Fixed(point) => point,
            Origin::Anchored(anchor) => anchor.resolve(measured, surface),
        }
    }
}

/// The surface origin with the whole surface available.
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

    const SURFACE: Rect = Rect::new(0.0, 0.0, 200.0, 100.0);
    const MEASURED: Size = Size::new(50.0, 30.0);

    /// A 4 gap below a 20x6 rect at (40, 10) puts the body's top at `10 + 6 + 4 = 20` and left at 40; neither flip nor clamp fires.
    fn anchored() -> Placement {
        Placement::default()
            .with_anchored(Anchor::below(Rect::new(40.0, 10.0, 20.0, 6.0)).with_gap(4.0))
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

    /// A cap bounds a fixed root inside the room its point leaves, whichever is smaller per axis,
    /// in either order: at (12, 7) the room is `200 − 12` by `100 − 7`, past the 80 by 40 cap; at
    /// (150, 0) it is `200 − 150 = 50` wide, under the cap of 100, which would run 50 past the edge.
    #[test]
    fn a_fixed_point_and_a_size_cap_land_the_same_way_in_either_order() {
        for (point, cap, want) in [
            (
                Vec2::new(12.0, 7.0),
                Size::new(80.0, 40.0),
                Size::new(80.0, 40.0),
            ),
            (
                Vec2::new(150.0, 0.0),
                Size::new(100.0, f32::INFINITY),
                Size::new(50.0, 100.0),
            ),
        ] {
            let point_first = anchored().with_fixed(point).with_max_size(cap);
            let size_first = anchored().with_max_size(cap).with_fixed(point);
            for placed in [point_first, size_first] {
                assert_eq!(placed.available(SURFACE), want, "{point} {cap:?}");
                assert_eq!(placed.origin(MEASURED, SURFACE), point);
            }
        }
    }

    /// Without a cap a fixed root gets the space from its point to the surface edge: `200 - 12` by `100 - 7`.
    #[test]
    fn an_uncapped_fixed_root_measures_against_the_rest_of_the_surface() {
        let placed = Placement::default().with_fixed(Vec2::new(12.0, 7.0));
        assert_eq!(placed.available(SURFACE), Size::new(188.0, 93.0));
    }
}
