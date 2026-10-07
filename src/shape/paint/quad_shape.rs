//! The quad-drawn shapes a record lowers to.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::shape::paint::lowered_shadow::LoweredShadow;
use crate::shape::paint::shape_brush::ShapeBrush;
use crate::shape::paint::shape_stroke::ShapeStroke;
use crate::shape::rect::RectKind;
use glam::Vec2;

/// Which shape a quad-tier draw paints: rectangle, box-shadow or rounded
/// triangle. All lower to a single `Quad` on one pipeline, selected by its
/// `fill_kind` lane, in [`crate::shape::lower`]. The composer's fast paths are
/// gated on `fill_kind == FillKind::SOLID`, which only [`Self::Rect`] carries.
/// All are owner-local.
#[derive(Clone, Copy, Debug)]
pub(crate) enum QuadShape {
    /// Filled or bordered rounded rectangle, or inverse window, by `kind`.
    /// `local_rect = None` covers the owner's arranged rect; `Some(r)` paints
    /// in owner-relative coords, in the slot it was pushed in, under the
    /// owner's clip but outside its pan transform.
    Rect {
        kind: RectKind,
        local_rect: Option<Rect>,
        corners: Corners,
        fill: ShapeBrush,
        border: ShapeStroke,
    },
    /// Gaussian-blurred rounded rect (drop or inset shadow). `local_rect =
    /// None` shadows the owner's arranged rect; the composer shifts a drop
    /// shadow by `offset` and inflates it by its
    /// [halo](crate::shape::paint::lowered_shadow::ShadowGeom::halo).
    Shadow {
        local_rect: Option<Rect>,
        corners: Corners,
        shadow: LoweredShadow,
    },
    /// Filled or bordered rounded triangle as an analytic SDF; `a`/`b`/`c` are
    /// owner-local, fills solid only. `bbox` is the AABB inflated by `radius`
    /// plus the AA fringe.
    Triangle {
        a: Vec2,
        b: Vec2,
        c: Vec2,
        radius: f32,
        /// Solid linear-RGB fill, straight alpha.
        fill: RgbaF16,
        border: ShapeStroke,
        bbox: Rect,
    },
}

impl QuadShape {
    /// Owner-local paint bbox, the basis for the screen-space paint bound.
    #[inline]
    pub(crate) fn bbox_local(&self, owner_size: Size) -> Rect {
        match self {
            QuadShape::Rect { local_rect, .. } => local_rect.unwrap_or(Rect {
                min: Vec2::ZERO,
                size: owner_size,
            }),
            QuadShape::Shadow {
                local_rect, shadow, ..
            } => shadow.paint_rect_local(*local_rect, owner_size),
            QuadShape::Triangle { bbox, .. } => *bbox,
        }
    }
}

/// `Triangle` tests its `bbox`; `radius` is checked separately, as lowering's
/// `radius.max(0.0)` launders a NaN to `0.0`.
impl NanCheck for QuadShape {
    fn has_nan(&self) -> bool {
        match self {
            Self::Rect {
                local_rect,
                corners,
                fill,
                border,
                ..
            } => local_rect.has_nan() || corners.has_nan() || fill.has_nan() || border.has_nan(),
            Self::Shadow {
                local_rect,
                corners,
                shadow,
            } => local_rect.has_nan() || corners.has_nan() || shadow.has_nan(),
            Self::Triangle {
                bbox,
                radius,
                fill,
                border,
                ..
            } => bbox.has_nan() || radius.is_nan() || fill.has_nan() || border.has_nan(),
        }
    }
}
