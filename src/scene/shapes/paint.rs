//! Lowered paint data shared by shape records and node chrome.

use crate::common::content_hash::ContentHash;
use crate::primitives::approx::noop_f32;
use crate::primitives::color::{Color, ColorF16};
use crate::primitives::corners::Corners;
use crate::primitives::half_simd::F16x4;
use crate::primitives::nan::NanCheck;
use crate::primitives::rect::Rect;
use crate::primitives::shadow::Shadow;
use crate::primitives::size::Size;
use crate::primitives::stroke::Stroke;
use crate::primitives::texture_id::TextureId;
use crate::scene::record_store::GradientId;
use crate::shape::rect::RectKind;
use glam::{UVec2, Vec2};

#[derive(Clone, Copy, Debug, Hash)]
pub(crate) enum ShapeBrush {
    Solid(ColorF16),
    Gradient(GradientId),
}

/// Lowered stroke: a straight-alpha colour and a logical-px width.
///
/// `width` leads so the `#[repr(C)]` layout has no interior padding —
/// which is what keeps the type `Pod`, and so hashable in one
/// `Hasher::pod` call by both `compute_record_hash` and
/// `lower::background`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ShapeStroke {
    pub(crate) width: f32,
    pub(crate) color: ColorF16,
}

impl ShapeStroke {
    /// The stroke a no-op normalizes to, and what a payload's
    /// "no stroke here" reads as.
    pub(crate) const NONE: Self = Self {
        width: 0.0,
        color: ColorF16::TRANSPARENT,
    };

    #[inline]
    pub(crate) fn is_noop(self) -> bool {
        noop_f32(self.width) || self.color.is_noop()
    }

    /// Collapse a no-op stroke to [`Self::NONE`]; pass anything else
    /// through verbatim.
    ///
    /// Every quad-tier draw normalizes here, and that is what lets
    /// `DrawQuadPayload::is_noop` test the stroke by colour alone: a
    /// transparent stroke colour in a payload means exactly "the stroke
    /// was a no-op".
    ///
    /// A NaN width normalizes away like any other non-painting width —
    /// `noop_f32` classifies it as invisible. Catching a NaN *loudly* is
    /// `Shape::debug_assert_no_nan`'s job, at the authoring boundary
    /// where the value still has a call site; by the time it reaches
    /// here the useful thing to do is fail safe.
    #[inline]
    pub(crate) fn normalized(self) -> Self {
        if self.is_noop() { Self::NONE } else { self }
    }
}

impl From<&Stroke> for ShapeStroke {
    #[inline]
    fn from(stroke: &Stroke) -> Self {
        Self {
            width: stroke.width,
            color: ColorF16::from(stroke.color),
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
        Stroke::solid(Color::from(stroke.color), stroke.width)
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
    /// Filled/stroked rounded rectangle or inverse window, selected by
    /// `kind`. With `local_rect = None` it covers the owner node's full
    /// arranged rect (position/size come from layout). With
    /// `local_rect = Some(r)` it paints `r` at owner-relative coords —
    /// `r.min = (0, 0)` is the owner's top-left. The sub-rect form
    /// paints in the slot it was pushed in (interleaved with children
    /// via the slot mechanism — see `Tree::add_shape`), still under the
    /// owner's clip but outside its pan transform. Used for scrollbar
    /// tracks/thumbs (pushed after body content → slot N) and TextEdit
    /// carets (pushed after the Text shape on a leaf → slot 0, after the
    /// Text in record order).
    Rect {
        kind: RectKind,
        local_rect: Option<Rect>,
        corners: Corners,
        fill: ShapeBrush,
        stroke: ShapeStroke,
        /// Pre-computed content hash of `fill` when it's a gradient,
        /// 0 for solid. Lets the record hash stay context-free —
        /// otherwise we'd need to thread the gradient payloads into
        /// every hash call (subtree rollups, measure cache). The hash
        /// is computed once at lowering time by [`super::lower::brush`].
        fill_grad_hash: u64,
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
    /// Filled/stroked rounded triangle, rendered as an analytic SDF
    /// (`FillKind::TRIANGLE`). `a`/`b`/`c` are owner-local corner
    /// points; the composer transforms them to physical px, packs them
    /// into the reused `Quad` corner/axis lanes, and the shader
    /// evaluates `sdf_triangle - radius` for rounded corners + coverage
    /// AA. Solid fill only (gradients don't fit the reused lanes).
    /// `bbox` is the owner-local AABB inflated by `radius + AA fringe`
    /// for damage / cull (the stroke is inner-edge, so it adds no
    /// outward reach).
    Triangle {
        a: Vec2,
        b: Vec2,
        c: Vec2,
        radius: f32,
        /// Solid linear-RGB fill (straight alpha).
        fill: ColorF16,
        stroke: ShapeStroke,
        bbox: Rect,
    },
}

impl QuadShape {
    /// Owner-local paint bbox — cascade's basis for the screen-space
    /// paint bound. A rectangle covers its `local_rect` (or the whole
    /// owner); a drop shadow reaches past its source by the halo
    /// [`shadow_paint_rect_local`] computes; a triangle carries the
    /// inflated hull lowering already derived.
    #[inline]
    pub(crate) fn bbox_local(&self, owner_size: Size) -> Rect {
        match self {
            QuadShape::Rect { local_rect, .. } => local_rect.unwrap_or(Rect {
                min: Vec2::ZERO,
                size: owner_size,
            }),
            QuadShape::Shadow {
                local_rect, shadow, ..
            } => {
                let ShadowGeom {
                    offset,
                    blur,
                    spread,
                } = shadow.geom();
                shadow_paint_rect_local(
                    *local_rect,
                    owner_size,
                    offset,
                    blur,
                    spread,
                    shadow.inset(),
                )
            }
            QuadShape::Triangle { bbox, .. } => *bbox,
        }
    }
}

/// Owner-local paint bbox of a [`QuadShape::Shadow`] — a drop shadow is
/// the offset source inflated by `3σ + max(spread, 0)`; an inset shadow
/// stays inside the source. `local_rect = None` ⇒ source covers the full
/// owner; `Some(r)` ⇒ source is `r` at owner-relative coords. **Sole formula
/// source** for the shadow paint extent: the encoder (per-quad paint rect) and
/// [`QuadShape::bbox_local`] (cascade's per-node ink union) both call
/// this so the two views can't drift.
pub(crate) fn shadow_paint_rect_local(
    local_rect: Option<Rect>,
    owner_size: Size,
    offset: Vec2,
    blur: f32,
    spread: f32,
    inset: bool,
) -> Rect {
    let source = match local_rect {
        None => Rect {
            min: Vec2::ZERO,
            size: owner_size,
        },
        Some(r) => r,
    };
    if inset {
        return source;
    }
    let halo = 3.0 * blur.max(0.0) + spread.max(0.0);
    Rect {
        min: source.min + offset,
        size: source.size,
    }
    .inflated(halo)
}

/// Where a textured rect samples from — the half that actually differs
/// between a registered image and an app-rendered GPU surface. Both
/// composite through the one image pipeline: same paint-rect
/// resolution, fit, sampling, tint, `DrawImagePayload`, and
/// `ImageInstance`. All that separates them is where the encoder gets
/// the `TextureId` and what drives their damage hash, so they are two
/// sources of one draw rather than two draws.
///
/// [`ShapeRecord::Image`]: crate::scene::shapes::record::ShapeRecord::Image
#[derive(Clone, Copy, Debug)]
pub(crate) enum ImageSource {
    /// A registered image. `id` is the registration id behind an
    /// [`ImageHandle`](crate::ImageHandle) — extracted at lowering so the
    /// per-frame record carries no `Rc` (the user's held handle is what
    /// keeps the GPU texture alive). The backend looks `id` up in its
    /// texture cache and skips the draw on a miss. `size` is the
    /// intrinsic dims, baked in at registration so the encoder reads
    /// them with no registry borrow.
    Texture { id: TextureId, size: UVec2 },
    /// An app-rendered GPU surface. The view's stable render-target
    /// `TextureId` and its `paint` callback live in `Ui::gpu_views`,
    /// keyed by the owner node's `WidgetId`, which the encoder reads to
    /// look the view up — kept off the record so the hot `records`
    /// buffer stays small and `Rc`-free. The encoder passes
    /// [`ImageFit`](crate::primitives::image::ImageFit) an all-zero
    /// intrinsic size, which resolves to the base rect at full UV.
    ///
    /// `epoch` is the view's damage version, folded into the shape hash
    /// (which only sees the record, so it rides here): `Ui::gpu_view`
    /// bumps it to the frame id on `repaint(true)` — the rect repaints
    /// and the texture re-renders — and holds it stable on
    /// `repaint(false)`, so a static view stays undamaged and is culled.
    GpuView { epoch: u64 },
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
    /// `(offset.x, offset.y, blur, spread)`. Wraps [`F16x4`] rather
    /// than a bare `[u16; 4]` for the reason that type exists: it is
    /// the shared 4-lane storage core, and a field that stores the
    /// lanes raw is a field that has to re-derive every lane idiom —
    /// pack, unpack, and the NaN screen — by hand.
    pub(crate) geom_f16: F16x4,
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
        // Geometry screened for NaN, not magnitude — a zero-sigma
        // zero-offset shadow still paints a hard-edged rect, so only
        // the tint's *size* decides visibility. Mirrors
        // `Shadow::is_noop` one tier up; needed separately because
        // chrome reaches `emit_shadow` through this lowered form, and
        // chrome has no record-level NaN gate behind it.
        self.color.is_noop() || self.geom_f16.has_nan()
    }

    #[inline]
    pub(crate) fn geom(self) -> ShadowGeom {
        let out = self.geom_f16.lanes();
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
        Self {
            color: shadow.color.into(),
            geom_f16: F16x4::from_lanes([
                shadow.offset.x,
                shadow.offset.y,
                shadow.blur,
                shadow.spread,
            ]),
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
impl NanCheck for ShapeStroke {
    #[inline]
    fn has_nan(&self) -> bool {
        self.width.is_nan() || self.color.has_nan()
    }
}

/// A gradient's geometry never reaches this far: `lower::brush` screens
/// it at intern time, so a `Gradient` here is known-finite and only its
/// solid sibling needs testing.
impl NanCheck for ShapeBrush {
    #[inline]
    fn has_nan(&self) -> bool {
        match self {
            Self::Solid(color) => color.has_nan(),
            Self::Gradient(_) => false,
        }
    }
}

impl NanCheck for LoweredShadow {
    #[inline]
    fn has_nan(&self) -> bool {
        self.color.has_nan() || self.geom_f16.has_nan()
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
                stroke,
                ..
            } => local_rect.has_nan() || corners.has_nan() || fill.has_nan() || stroke.has_nan(),
            Self::Shadow {
                local_rect,
                corners,
                shadow,
            } => local_rect.has_nan() || corners.has_nan() || shadow.has_nan(),
            Self::Triangle {
                bbox,
                radius,
                fill,
                stroke,
                ..
            } => bbox.has_nan() || radius.is_nan() || fill.has_nan() || stroke.has_nan(),
        }
    }
}
