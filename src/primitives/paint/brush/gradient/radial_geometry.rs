//! The radial gradient's axis: color runs outward from a center, with a radius per axis for ellipses.

use crate::primitives::math::float_hash::FloatHash;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::brush::gradient::Gradient;
use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::paint::brush::gradient::sealed::Geometry;
use glam::Vec2;
use std::hash;

/// Geometry of a radial gradient: color runs outward from `center` along the elliptical `radius`, both in object-space 0..1 (origin top-left). The shader projects to `t = length((local01 - center) / radius)`, applies `Spread`, samples the LUT.
#[derive(Clone, Copy, Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RadialGeometry {
    /// Ramp origin, in object-space `0..1` coordinates.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::offset2")]
    pub center: Vec2,
    /// Elliptical radius, per axis, in the same coordinates. A `Vec2`, not a
    /// [`Size`](crate::Size): a pair of semi-axes the projection divides by, in object space, as
    /// kurbo's `Ellipse::radii` is, where a `Size` is an extent in logical px with an unbounded
    /// sentinel.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length2")]
    pub radius: Vec2,
}

/// Radial gradient — see [`RadialGeometry`] for the projection.
pub type RadialGradient = Gradient<RadialGeometry>;

impl Geometry for RadialGeometry {
    /// The shader reads these as `(cx, cy, rx, ry)` on the radial branch.
    fn axis_lanes(&self) -> [f32; 4] {
        [self.center.x, self.center.y, self.radius.x, self.radius.y]
    }

    fn hash_geometry<H: hash::Hasher>(&self, state: &mut H) {
        self.center.hash_visual(state);
        self.radius.hash_visual(state);
    }

    fn has_nan(&self) -> bool {
        self.center.has_nan() || self.radius.has_nan()
    }
}

impl RadialGradient {
    /// A gradient out from `center` to `radius` through `ramp`: `ramp` an array of colors
    /// spread evenly, an array of [`Stop`](crate::Stop)s placed where they say, or a [`ColorRamp`].
    ///
    /// # Panics
    ///
    /// As [`ColorRamp::new`](crate::ColorRamp::new).
    #[track_caller]
    pub fn new(center: Vec2, radius: Vec2, ramp: impl Into<ColorRamp>) -> Self {
        Self::from_ramp(RadialGeometry { center, radius }, ramp.into())
    }

    /// The inscribed ellipse: from the center out to each side's midpoint.
    ///
    /// # Panics
    ///
    /// As [`Self::new`].
    #[track_caller]
    pub fn centered(ramp: impl Into<ColorRamp>) -> Self {
        Self::new(Vec2::splat(0.5), Vec2::splat(0.5), ramp)
    }
}
