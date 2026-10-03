//! The quad-drawn shapes a record lowers to.

use crate::primitives::color::rgba_f16::RgbaF16;
use crate::primitives::corners::Corners;
use crate::primitives::nan::NanCheck;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::scene::shapes::paint::lowered_shadow::LoweredShadow;
use crate::scene::shapes::paint::shape_brush::ShapeBrush;
use crate::scene::shapes::paint::shape_stroke::ShapeStroke;
use crate::shape::rect::RectKind;
use glam::Vec2;

/// Which shape a quad-tier draw paints — the half that actually differs
/// between a rectangle, a box-shadow, and a rounded triangle. Named for
/// the shader's own vocabulary: all three lower to a single `Quad` on
/// the one quad pipeline, selected by its `fill_kind` lane, so they are
/// three shapes of one draw rather than three draws.
///
/// Lowered once, in [`crate::scene::shapes::lower`], and then carried
/// from [`ShapeRecord::Quad`] through `DrawQuadPayload` to the composer
/// — the tiers in between share the cull, group-flush, instance push,
/// and occlusion handling and never re-split the three forms. The
/// composer's three fast paths (clear fold, fragment fast bit, opaque
/// occluder) are all gated on `fill_kind == FillKind::SOLID`, which only
/// [`Self::Rect`] can carry, so sharing that code path is what keeps
/// shadows and triangles out of them rather than a per-shape branch.
///
/// All three are owner-local; the encoder resolves them against the
/// owner's arranged rect on the way to a payload.
///
/// [`ShapeRecord::Quad`]: crate::scene::shapes::record::ShapeRecord::Quad
#[derive(Clone, Copy, Debug)]
pub(crate) enum QuadShape {
    /// Filled/bordered rounded rectangle or inverse window, selected by
    /// `kind`. With `local_rect = None` it covers the owner node's full
    /// arranged rect (position/size come from layout). With
    /// `local_rect = Some(r)` it paints `r` at owner-relative coords —
    /// `r.min = (0, 0)` is the owner's top-left. The sub-rect form
    /// paints in the slot it was pushed in (interleaved with children
    /// via the slot mechanism — see `Forest::add_shape`), still under the
    /// owner's clip but outside its pan transform. Used for scrollbar
    /// tracks/thumbs (pushed after body content → slot N) and TextEdit
    /// carets (pushed after the Text shape on a leaf → slot 0, after the
    /// Text in record order).
    Rect {
        kind: RectKind,
        local_rect: Option<Rect>,
        corners: Corners,
        fill: ShapeBrush,
        border: ShapeStroke,
    },
    /// Gaussian-blurred rounded rect — drop / inset shadow. All
    /// parameters are inline scalars; no retained payloads. With
    /// `local_rect = None` the shadow shadows the owner's full arranged
    /// rect; with `Some(r)` it shadows the owner-relative rect `r`. The
    /// encoder shifts drop-shadow paint bounds by `offset`, inflates
    /// them by `3σ + max(spread, 0)`, and routes both shadow kinds
    /// through `FillKind::SHADOW_DROP|SHADOW_INSET`.
    Shadow {
        local_rect: Option<Rect>,
        corners: Corners,
        shadow: LoweredShadow,
    },
    /// Filled/bordered rounded triangle, rendered as an analytic SDF
    /// (`FillKind::TRIANGLE`). `a`/`b`/`c` are owner-local corner
    /// points; the composer transforms them to physical px, packs them
    /// into the reused `Quad` corner/axis lanes, and the shader
    /// evaluates `sdf_triangle - radius` for rounded corners + coverage
    /// AA. Solid fill only (gradients don't fit the reused lanes).
    /// `bbox` is the owner-local AABB inflated by `radius + AA fringe`
    /// for damage / cull (the border is inside the edge, so it adds no
    /// outward reach).
    Triangle {
        a: Vec2,
        b: Vec2,
        c: Vec2,
        radius: f32,
        /// Solid linear-RGB fill (straight alpha).
        fill: RgbaF16,
        border: ShapeStroke,
        bbox: Rect,
    },
}

impl QuadShape {
    /// Owner-local paint bbox — cascade's basis for the screen-space
    /// paint bound. A rectangle covers its `local_rect` (or the whole
    /// owner); a drop shadow reaches past its source by the halo
    /// [`LoweredShadow::paint_rect_local`] computes; a triangle carries
    /// the inflated hull lowering already derived.
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

/// `Triangle` tests its `bbox` rather than `a`/`b`/`c`, which lowering
/// folds through `Aabb` under the AABB NaN contract — so one `Rect` test
/// covers the three corners. `radius` is **not** among them: lowering
/// only reaches it through `radius.max(0.0)`, which launders a NaN to
/// `0.0`, so it is named separately here.
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
