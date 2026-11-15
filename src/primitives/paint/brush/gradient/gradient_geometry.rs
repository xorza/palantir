//! The per-kind half of a gradient: the geometry the shader projects a fragment onto.

use crate::primitives::math::domain::{self, vec2};
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::brush::gradient::conic_geometry::ConicGeometry;
use crate::primitives::paint::brush::gradient::linear_geometry::LinearGeometry;
use crate::primitives::paint::brush::gradient::radial_geometry::RadialGeometry;
use std::hash;

/// Which kind a [`Gradient`](crate::Gradient) is, with the geometry of that kind: the renderer
/// draws exactly these three.
#[derive(Clone, Copy, Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub enum GradientGeometry {
    /// Along an angle.
    Linear(LinearGeometry),
    /// Out from a center along an elliptical radius.
    Radial(RadialGeometry),
    /// Around a center by angle.
    Conic(ConicGeometry),
}

impl GradientGeometry {
    /// The four axis lanes the shader reads, before `FillAxis` packs them to f16.
    pub(crate) fn axis_lanes(&self) -> [f32; 4] {
        match self {
            Self::Linear(g) => g.axis_lanes(),
            Self::Radial(g) => g.axis_lanes(),
            Self::Conic(g) => g.axis_lanes(),
        }
    }

    /// Folds the kind and the geometry into a cache key; f32 fields hash through
    /// `float_hash::canon_bits` so `-0.0`/`+0.0` and NaN patterns don't fragment dedup.
    pub(crate) fn hash_visual<H: hash::Hasher>(&self, state: &mut H) {
        match self {
            Self::Linear(g) => {
                state.write_u8(0);
                g.hash_visual(state);
            }
            Self::Radial(g) => {
                state.write_u8(1);
                g.hash_visual(state);
            }
            Self::Conic(g) => {
                state.write_u8(2);
                g.hash_visual(state);
            }
        }
    }

    /// Whether the geometry holds its kinds: [`Self::validate`]'s test.
    #[inline]
    pub(crate) const fn is_valid(&self) -> bool {
        match self {
            Self::Linear(g) => domain::is_angle(g.angle),
            Self::Radial(RadialGeometry { center, radius }) => {
                domain::are_lanes([center.x, center.y])
                    & domain::are_length_lanes([radius.x, radius.y])
            }
            Self::Conic(ConicGeometry {
                center,
                start_angle,
            }) => domain::are_lanes([center.x, center.y]) & domain::is_angle(*start_angle),
        }
    }

    /// Panics unless the geometry holds its kinds ([`domain`]). A center and a radius pack into
    /// f16 lanes, so each is a [lane](domain::lane) too.
    #[inline]
    #[track_caller]
    pub(crate) const fn validate(&self) {
        match self {
            Self::Linear(g) => {
                domain::angle(g.angle);
            }
            Self::Radial(g) => {
                vec2::lane(vec2::offset(g.center));
                vec2::lane(vec2::length(g.radius));
            }
            Self::Conic(g) => {
                vec2::lane(vec2::offset(g.center));
                domain::angle(g.start_angle);
            }
        }
    }
}

impl NanCheck for GradientGeometry {
    #[inline]
    fn has_nan(&self) -> bool {
        match self {
            Self::Linear(g) => g.angle.is_nan(),
            Self::Radial(g) => g.center.has_nan() || g.radius.has_nan(),
            Self::Conic(g) => g.center.has_nan() || g.start_angle.is_nan(),
        }
    }
}
