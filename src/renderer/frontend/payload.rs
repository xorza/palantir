//! Lowered paint payloads — the values the encoder hands a
//! [`PaintSink`](crate::renderer::frontend::paint_sink::PaintSink),
//! one per paint operation.
//!
//! Plain value types: they used to be `bytemuck::Pod` so a packed
//! command arena could store them, and carried `#[repr(C)]` plus
//! injected trailing padding to satisfy that. Nothing serializes them
//! now, so the layout is the compiler's to choose and fields can be
//! ordinary enums rather than `u8` newtypes.

use crate::primitives::approx::noop_f32;
use crate::primitives::brush::gradient::FillAxis;
use crate::primitives::fill_wire::{FillKind, LutRow};
use crate::primitives::texture_id::TextureId;
use crate::primitives::{
    color::{Color, ColorF16},
    corners::Corners,
    rect::Rect,
};
use crate::scene::shapes::record::ColorMode;
use crate::shape::style::{LineCap, LineJoin};
use crate::text::shaped_ref::ShapedTextRef;

/// Physical gradient identity resolved for this encode pass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ResolvedGradient {
    pub(crate) axis: FillAxis,
    pub(crate) row: LutRow,
    pub(crate) kind: FillKind,
}

/// Lowered brush input. `Solid` carries an 8-byte `ColorF16`;
/// `Gradient` carries the 16-byte atlas row + axis + kind resolved for
/// this encode pass.
#[derive(Clone, Copy, Debug)]
pub(crate) enum BrushSource {
    Solid(ColorF16),
    Gradient(ResolvedGradient),
}

impl BrushSource {
    /// `Gradient.is_noop()` is always `false` — the all-transparent-
    /// stops case is filtered by `Brush::is_noop` *before* lowering,
    /// and the lowered form drops the stops. A gradient slipping past
    /// the upstream gate would paint a useless transparent quad; the
    /// alpha blend produces nothing visible, so correctness is intact.
    #[inline]
    pub(crate) fn is_noop(self) -> bool {
        match self {
            Self::Solid(c) => c.is_noop(),
            Self::Gradient(_) => false,
        }
    }

    /// Lower to the GPU fill fields shared by every draw-rect/curve
    /// payload: a `Solid` carries its colour with the `SOLID` kind and
    /// the magenta fallback row; a `Gradient` zeroes the colour (the
    /// atlas row supplies it) and forwards kind/row/axis.
    #[inline]
    pub(crate) fn to_gpu_fields(self) -> GpuFillFields {
        match self {
            Self::Solid(c) => GpuFillFields {
                color: c,
                kind: FillKind::SOLID,
                lut_row: LutRow::FALLBACK,
                axis: FillAxis::ZERO,
            },
            Self::Gradient(g) => GpuFillFields {
                color: ColorF16::TRANSPARENT,
                kind: g.kind,
                lut_row: g.row,
                axis: g.axis,
            },
        }
    }
}

/// GPU fill fields a [`BrushSource`] lowers to. Curve payloads carry no
/// `axis`, so they read only the first three.
#[derive(Clone, Copy, Debug)]
pub(crate) struct GpuFillFields {
    pub(crate) color: ColorF16,
    pub(crate) kind: FillKind,
    pub(crate) lut_row: LutRow,
    pub(crate) axis: FillAxis,
}

/// Scissor clip payload. `corners` is all-zero for plain rect clips
/// and non-zero for rounded-mask clips — the composer decides which
/// path to take by inspecting it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PushClipPayload {
    pub(crate) rect: Rect,
    pub(crate) corners: Corners,
}

/// Brush metadata packed into draw-rect payloads. `fill_kind` low byte
/// is the kind tag; bits 8..16 carry `Spread` for gradient variants.
/// `fill_lut_row` is the pre-registered gradient atlas row (set at
/// shape lowering time), or [`LutRow::FALLBACK`] for solid fills.
/// `fill_axis` carries gradient geometry packed at lowering. `fill:
/// ColorF16` is the solid colour when `kind == SOLID`; zeroed for
/// gradients (the atlas row supplies the colour). Storing as
/// `ColorF16` (8 B linear-RGB) vs. 16 B `Color` saves 8 B per rect
/// payload — the composer decodes via `Color::from(f16)` at `Quad`
/// write time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawRectPayload {
    pub(crate) rect: Rect,
    pub(crate) corners: Corners,
    /// Linear-RGB fill (straight alpha). Zeroed for gradients; the
    /// atlas row at `fill_lut_row` supplies the colour in that case.
    pub(crate) fill: ColorF16,
    pub(crate) stroke_color: ColorF16,
    pub(crate) stroke_width: f32,
    pub(crate) fill_kind: FillKind,
    pub(crate) fill_lut_row: LutRow,
    pub(crate) fill_axis: FillAxis,
}

/// Box-shadow paint payload. A drop-shadow `rect` is the offset source
/// inflated by `3σ + max(spread, 0)`; an inset-shadow `rect` is the source.
/// `corners` carries the *source* shape's corner radii. `color` is the
/// shadow tint. `fill_kind` is `FillKind::SHADOW_DROP` or
/// `SHADOW_INSET`. `fill_axis` carries `(0, 0, σ, spread)` for drops and
/// `(offset.x, offset.y, σ, spread)` for insets in logical px; the
/// composer scales these to physical px so the shader's `local` coords line up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawShadowPayload {
    pub(crate) rect: Rect,
    pub(crate) corners: Corners,
    pub(crate) color: ColorF16,
    pub(crate) fill_kind: FillKind,
    pub(crate) fill_axis: FillAxis,
}

impl DrawShadowPayload {
    /// Canonical noop predicate — zero-extent paint rect or fully
    /// transparent tint. Shadow params themselves (`fill_axis`) are
    /// not gated: a zero-σ drop shadow can still paint a hard-edged
    /// shifted rect; the `Shape::Shadow::is_noop`
    /// authoring boundary catches the "no visible effect" cases.
    #[inline]
    pub(crate) fn is_noop(&self) -> bool {
        self.rect.is_paint_empty() || self.color.is_noop()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawTextPayload {
    pub(crate) rect: Rect,
    pub(crate) color: ColorF16,
    pub(crate) text: ShapedTextRef,
}

impl DrawTextPayload {
    /// Canonical noop predicate for this payload — zero-extent rect
    /// or fully transparent color. See [`PaintSink`](crate::renderer::frontend::paint_sink::PaintSink)
    /// for the noop policy.
    #[inline]
    pub(crate) fn is_noop(&self) -> bool {
        self.rect.is_paint_empty() || self.color.is_noop()
    }
}

/// Stroked polyline payload. `width` is logical px. Points + colors
/// live in the window's [`RecordPayloads`] (`polyline_points` /
/// `polyline_colors`) — the payload only carries the spans.
/// `colors_len` is 1 (broadcast), `points_len` (per-point), or
/// `points_len - 1` (per-segment), selected by `color_mode`.
///
/// Points are stored **owner-local**; the composer applies `origin`
/// (the owner-rect top-left) before the active push-transform stack.
/// `bbox` is their owner-local centerline AABB; the composer applies
/// stroke/cap/join/AA inflation once in physical space.
///
/// [`RecordPayloads`]: crate::scene::record_store::RecordPayloads
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(crate) struct DrawPolylinePayload {
    pub(crate) bbox: Rect,
    pub(crate) origin: glam::Vec2,
    pub(crate) width: f32,
    /// Paint-time rotation (radians) about `bbox.center()` (owner-local),
    /// applied to each point before the ancestor transform. `0.0` = none,
    /// the common case. Set from a [`PaintAnim::Spin`] sample; the
    /// encoder widens the centerline `bbox` to the rotation-invariant
    /// owner box so the scissor cull stays correct and its centre is
    /// the spin pivot.
    ///
    /// [`PaintAnim::Spin`]: crate::scene::tree::paint_anims::PaintAnim::Spin
    pub(crate) rotation: f32,
    pub(crate) points_start: u32,
    pub(crate) points_len: u32,
    pub(crate) colors_start: u32,
    pub(crate) colors_len: u32,
    pub(crate) color_mode: ColorMode,
    pub(crate) cap: LineCap,
    pub(crate) join: LineJoin,
}

impl DrawPolylinePayload {
    /// Canonical noop predicate — fewer than two points (no
    /// segments) or a non-paintable stroke width. **Does not** check
    /// color noop-ness: per-point / per-segment colours live in
    /// spans on the record store, and an O(n) read here would
    /// dominate the per-call cost. Color noop is filtered at the
    /// `Shape::Polyline::is_noop` authoring boundary instead. The
    /// bbox can legitimately be zero-area (horizontal / vertical
    /// line) and still paint stroke pixels, so it's not gated either.
    #[inline]
    pub(crate) fn is_noop(&self) -> bool {
        self.points_len < 2 || noop_f32(self.width)
    }
}

/// Mesh draw payload. Vertex/index data lives in the window's
/// [`RecordPayloads`] (`meshes`); the payload only carries the spans
/// (owner-local). The composer folds `origin` (owner-rect top-left)
/// into the per-instance translate so the vertex stream stays
/// content-stable across frames.
///
/// [`RecordPayloads`]: crate::scene::record_store::RecordPayloads
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawMeshPayload {
    /// Owner-local AABB of `vertices`. The composer transforms the
    /// four corners (uniform-scale `TranslateScale` preserves AABBs)
    /// after adding `origin`, scales to physical px, and uses the
    /// result for the overlap test + scissor cull.
    pub(crate) bbox: Rect,
    pub(crate) origin: glam::Vec2,
    pub(crate) tint: ColorF16,
    pub(crate) v_start: u32,
    pub(crate) v_len: u32,
    pub(crate) i_start: u32,
    pub(crate) i_len: u32,
}

impl DrawMeshPayload {
    /// Canonical noop predicate — empty vertex buffer, fewer than
    /// one full triangle, an index count that isn't a multiple of 3,
    /// or fully transparent tint.
    #[inline]
    pub(crate) fn is_noop(&self) -> bool {
        self.v_len == 0 || self.i_len < 3 || !self.i_len.is_multiple_of(3) || self.tint.is_noop()
    }
}

/// Image draw payload. `rect` is the logical-px paint rect (encoder
/// already folded in `local_rect`, `fit`, and the image's intrinsic
/// size). `uv_min` / `uv_size` are the texture crop — `(0,0)`+`(1,1)`
/// for the common Fill/Contain/None modes; non-trivial only for Cover.
/// `tint` multiplies the sampled texel. `handle` is the user-supplied
/// [`ImageHandle`](crate::renderer::image_registry::ImageHandle) — the
/// backend looks it up against its GPU texture
/// cache.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawImagePayload {
    pub(crate) rect: Rect,
    pub(crate) uv_min: glam::Vec2,
    pub(crate) uv_size: glam::Vec2,
    pub(crate) tint: ColorF16,
    /// The image's registration id ([`TextureId`],
    /// a `repr(transparent)` `Pod` `u64`). The backend looks it up in its
    /// texture cache; `TextureId(0)` (the `Zeroable` default) is "no
    /// texture" and skips the draw.
    pub(crate) handle: TextureId,
    /// `IMG_FLAG_*` bits (tile wrap, min/mag nearest sampling), forwarded
    /// verbatim into [`ImageInstance::flags`](crate::renderer::render_buffer::image::ImageInstance).
    /// `0` (the common case, including a `GpuView`) samples the UV
    /// directly with the bilinear sampler.
    pub(crate) flags: u32,
    /// Whether this draw composites a `GpuView`'s off-screen target
    /// rather than a registered image. Private — set only through
    /// [`Self::image`] / [`Self::gpu_view`]. The sink receives the paint
    /// callback alongside the payload; this only tells [`Self::is_noop`]
    /// not to null-skip a framework-painted texture. `handle` carries the
    /// view's stable `TextureId` either way, so the draw + cache path
    /// stays identical to an image.
    gpu_view: bool,
}

impl DrawImagePayload {
    /// An ordinary image draw — no off-screen target.
    #[inline]
    pub(crate) fn image(
        rect: Rect,
        uv_min: glam::Vec2,
        uv_size: glam::Vec2,
        tint: ColorF16,
        handle: TextureId,
        flags: u32,
    ) -> Self {
        Self {
            rect,
            uv_min,
            uv_size,
            tint,
            handle,
            flags,
            gpu_view: false,
        }
    }

    /// A `GpuView` composite over its full arranged `rect`: full UV,
    /// untinted, sampling the view's stable `handle`.
    #[inline]
    pub(crate) fn gpu_view(rect: Rect, handle: TextureId) -> Self {
        Self {
            rect,
            uv_min: glam::Vec2::ZERO,
            uv_size: glam::Vec2::ONE,
            tint: ColorF16::from(Color::WHITE),
            handle,
            flags: 0,
            gpu_view: true,
        }
    }

    /// Canonical noop predicate — zero-extent rect, fully transparent tint,
    /// or null handle (paints no pixels, no texture to sample). A
    /// `GpuView` is never null-skipped — its texture is framework-painted
    /// this frame, not a registered image that could have been dropped.
    #[inline]
    pub(crate) fn is_noop(&self) -> bool {
        self.rect.is_paint_empty() || self.tint.is_noop() || (self.handle.0 == 0 && !self.gpu_view)
    }
}

/// Which parametric basis a stroke traces — the payload half that
/// actually differs between a Bézier and an arc. Named for the shader's
/// own vocabulary: both lower to a `CurveInstance` on the one curve
/// pipeline, selected by its `kind` lane, so they are two bases of one
/// draw rather than two draws.
///
/// Both forms are owner-local. The composer folds in `origin` and the
/// active transform before scaling to physical px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CurveBasis {
    /// Cubic Bézier control points. Quadratics promote to cubic and
    /// straight lines degenerate to one at lowering, so this is every
    /// non-circular stroke.
    Cubic {
        p0: glam::Vec2,
        p1: glam::Vec2,
        p2: glam::Vec2,
        p3: glam::Vec2,
    },
    /// Exact circle — no cubic approximation error, and gradient `t`
    /// tracks the sweep linearly. `a0`/`a1` are radians in the screen
    /// convention (0 = +x, y-down ⇒ increasing = clockwise), so
    /// `a1 < a0` is a negative sweep.
    Arc {
        center: glam::Vec2,
        radius: f32,
        a0: f32,
        a1: f32,
    },
}

impl Default for CurveBasis {
    /// A degenerate cubic at the origin — the `Default` a
    /// [`DrawCurvePayload`] literal falls back to, never a real draw.
    fn default() -> Self {
        Self::Cubic {
            p0: glam::Vec2::ZERO,
            p1: glam::Vec2::ZERO,
            p2: glam::Vec2::ZERO,
            p3: glam::Vec2::ZERO,
        }
    }
}

/// Native GPU stroke payload — a cubic or an arc, per [`CurveBasis`].
/// The composer adds `origin` and the active push-transform stack
/// before scaling to physical px and pushing the resulting
/// `CurveInstance`(s) onto `RenderBuffer.curves`. `bbox` is the
/// owner-local centerline AABB; the composer applies the shared
/// stroke/cap/AA bound in physical space for culling and overlap.
/// `rotation` is the paint-time spin angle sampled from
/// `PaintAnim::Spin` — non-zero only when the encoder replaced `bbox`
/// with the rotation-invariant square whose centre is the spin pivot
/// (same contract as `DrawPolylinePayload`). The composer spins about
/// that pivot: exact for a Bézier by affine invariance, and for a
/// circle by rotating the centre and shifting both angles.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(crate) struct DrawCurvePayload {
    pub(crate) basis: CurveBasis,
    pub(crate) bbox: Rect,
    pub(crate) origin: glam::Vec2,
    pub(crate) rotation: f32,
    /// Solid stroke colour. Zeroed when `fill_kind` is a gradient —
    /// the LUT row at `fill_lut_row` supplies the colour in that case.
    pub(crate) color: ColorF16,
    pub(crate) width: f32,
    /// Typed Pod wire form; composer widens it only at the GPU
    /// `CurveInstance.cap` boundary.
    pub(crate) cap: LineCap,
    /// Brush kind tag (low byte: 0 = solid, 1 = linear). Only solid +
    /// linear are valid on curves; the lowering hard-asserts.
    pub(crate) fill_kind: FillKind,
    /// Gradient atlas row when `fill_kind` is a gradient, else
    /// [`LutRow::FALLBACK`].
    pub(crate) fill_lut_row: LutRow,
}

/// Rounded-triangle payload. The three corner points `a`/`b`/`c` are stored
/// **owner-local**; the composer folds in `origin` (owner-rect top-left) + the
/// active push-transform before scaling to physical px, then derives the
/// covering AABB (from the points inflated by `radius + AA fringe`) and packs
/// the physical points into a `Quad` with `FillKind::TRIANGLE`.
/// `fill` is the solid fill; `stroke_color` / `stroke_width` the inner-edge
/// stroke. `radius` rounds all three corners (`0.0` = sharp).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawTrianglePayload {
    pub(crate) origin: glam::Vec2,
    pub(crate) a: glam::Vec2,
    pub(crate) b: glam::Vec2,
    pub(crate) c: glam::Vec2,
    /// Solid linear-RGB fill (straight alpha).
    pub(crate) fill: ColorF16,
    pub(crate) stroke_color: ColorF16,
    pub(crate) radius: f32,
    pub(crate) stroke_width: f32,
}

impl DrawTrianglePayload {
    /// Canonical noop predicate — nothing paints when the fill is
    /// transparent *and* the stroke is a no-op (transparent or zero width).
    #[inline]
    pub(crate) fn is_noop(&self) -> bool {
        self.fill.is_noop() && (self.stroke_color.is_noop() || noop_f32(self.stroke_width))
    }
}

impl DrawCurvePayload {
    /// Canonical noop predicate — zero/negative stroke width, a
    /// degenerate arc radius (nothing to trace), or a solid fill that's
    /// fully transparent. Gradient fills always paint (the
    /// all-transparent-stops case is caught by `Brush::is_noop` before
    /// lowering).
    #[inline]
    pub(crate) fn is_noop(&self) -> bool {
        if noop_f32(self.width) {
            return true;
        }
        if let CurveBasis::Arc { radius, .. } = self.basis
            && noop_f32(radius)
        {
            return true;
        }
        self.fill_kind == FillKind::SOLID && self.color.is_noop()
    }
}
