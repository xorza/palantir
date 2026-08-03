//! Lowered paint data shared by shape records and node chrome.

use crate::common::content_hash::ContentHash;
use crate::primitives::color::{Color, ColorF16};
use crate::primitives::corners::Corners;
use crate::primitives::shadow::Shadow;
use crate::primitives::stroke::Stroke;
use crate::scene::record_store::GradientId;
use glam::Vec2;
use half::f16;

#[derive(Clone, Copy, Debug, Hash)]
pub(crate) enum ShapeBrush {
    Solid(ColorF16),
    Gradient(GradientId),
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ShapeStroke {
    pub(crate) color: ColorF16,
    pub(crate) width_f16: u16,
}

impl ShapeStroke {
    #[inline]
    pub(crate) fn width(self) -> f32 {
        f16::from_bits(self.width_f16).to_f32()
    }

    #[inline]
    pub(crate) fn is_noop(self) -> bool {
        use crate::primitives::approx::noop_f16_bits;
        noop_f16_bits(self.width_f16) || self.color.is_noop()
    }
}

impl From<&Stroke> for ShapeStroke {
    #[inline]
    fn from(stroke: &Stroke) -> Self {
        Self {
            color: ColorF16::from(stroke.color),
            width_f16: f16::from_f32(stroke.width).to_bits(),
        }
    }
}

impl From<Stroke> for ShapeStroke {
    #[inline]
    fn from(stroke: Stroke) -> Self {
        Self::from(&stroke)
    }
}

impl From<ShapeStroke> for Stroke {
    #[inline]
    fn from(stroke: ShapeStroke) -> Self {
        Stroke::solid(Color::from(stroke.color), stroke.width())
    }
}

/// Which parametric basis a stroke traces — the half that actually
/// differs between a Bézier and an arc. Named for the shader's own
/// vocabulary: both lower to a `CurveInstance` on the one curve
/// pipeline, selected by its `kind` lane, so they are two bases of one
/// draw rather than two draws.
///
/// Lowered once, in [`crate::scene::shapes::lower`], and then carried
/// verbatim from [`ShapeRecord::Curve`] through `DrawCurvePayload` to
/// the composer — the tiers in between share the stroke's width, cap,
/// fill, and bbox handling and never re-split the two forms.
///
/// Both forms are owner-local. The composer folds in the owner origin
/// and the active transform before scaling to physical px.
///
/// [`ShapeRecord::Curve`]: crate::scene::shapes::record::ShapeRecord::Curve
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CurveBasis {
    /// Cubic Bézier control points. Quadratics promote to cubic and
    /// straight lines degenerate to one at lowering, so this is every
    /// non-circular stroke.
    Cubic {
        p0: Vec2,
        p1: Vec2,
        p2: Vec2,
        p3: Vec2,
    },
    /// Exact circle — no cubic approximation error, and gradient `t`
    /// tracks the sweep linearly. `a0`/`a1` are radians in the screen
    /// convention (0 = +x, y-down ⇒ increasing = clockwise), so
    /// `a1 < a0` is a negative sweep.
    Arc {
        center: Vec2,
        radius: f32,
        a0: f32,
        a1: f32,
    },
}

impl CurveBasis {
    /// Stable hash tag distinguishing the two bases, written by
    /// [`compute_record_hash`] ahead of the basis fields. Both bases
    /// share `ShapeRecord::Curve`'s tag, so without this a cubic and an
    /// arc would be told apart only by how many floats they happen to
    /// feed the hasher. Frozen for the same reason
    /// [`ShapeRecord::tag`] is: these numbers reach cached hashes.
    ///
    /// [`compute_record_hash`]: crate::scene::shapes::hash::compute_record_hash
    /// [`ShapeRecord::tag`]: crate::scene::shapes::record::ShapeRecord::tag
    #[inline]
    pub(crate) const fn tag(&self) -> u8 {
        match self {
            CurveBasis::Cubic { .. } => 0,
            CurveBasis::Arc { .. } => 1,
        }
    }
}

impl Default for CurveBasis {
    /// A degenerate cubic at the origin — the `Default` a
    /// `DrawCurvePayload` literal falls back to, never a real draw.
    fn default() -> Self {
        Self::Cubic {
            p0: Vec2::ZERO,
            p1: Vec2::ZERO,
            p2: Vec2::ZERO,
            p3: Vec2::ZERO,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ChromeRow {
    pub(crate) fill: ShapeBrush,
    pub(crate) stroke: ShapeStroke,
    pub(crate) corners: Corners,
    pub(crate) shadow: LoweredShadow,
    pub(crate) hash: ContentHash,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct LoweredShadow {
    pub(crate) color: ColorF16,
    pub(crate) geom_f16: [u16; 4],
    pub(crate) inset_flag: u16,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ShadowGeom {
    pub(crate) offset: Vec2,
    pub(crate) blur: f32,
    pub(crate) spread: f32,
}

impl LoweredShadow {
    #[inline]
    pub(crate) fn is_noop(self) -> bool {
        self.color.is_noop()
    }

    #[inline]
    pub(crate) fn geom(self) -> ShadowGeom {
        use crate::primitives::half_simd::f16x4_to_f32x4;
        let out = f16x4_to_f32x4(self.geom_f16);
        ShadowGeom {
            offset: Vec2::new(out[0], out[1]),
            blur: out[2],
            spread: out[3],
        }
    }

    #[inline]
    pub(crate) fn inset(self) -> bool {
        self.inset_flag != 0
    }
}

impl From<Shadow> for LoweredShadow {
    #[inline]
    fn from(shadow: Shadow) -> Self {
        use crate::primitives::half_simd::f16x4_from_f32x4;
        let geom_f16 =
            f16x4_from_f32x4([shadow.offset.x, shadow.offset.y, shadow.blur, shadow.spread]);
        Self {
            color: shadow.color.into(),
            geom_f16,
            inset_flag: shadow.inset as u16,
        }
    }
}

impl std::hash::Hash for LoweredShadow {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write(bytemuck::bytes_of(self));
    }
}
