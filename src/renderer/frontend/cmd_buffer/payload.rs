//! Typed command discriminants and Pod payload records.

use crate::primitives::approx::noop_f32;
use crate::primitives::brush::FillAxis;
use crate::primitives::fill_wire::{FillKind, LutRow};
use crate::primitives::{
    color::{Color, ColorF16},
    corners::Corners,
    rect::Rect,
    transform::TranslateScale,
};
use crate::renderer::texture_id::TextureId;
use crate::shape::{ColorModeBits, LineCapBits, LineJoinBits};
use crate::text::TextCacheKey;

/// Physical gradient identity resolved for this encode pass.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ResolvedGradient {
    pub(crate) axis: FillAxis,
    pub(crate) row: LutRow,
    pub(crate) kind: FillKind,
}

/// Cmd-buffer brush input. `Solid` carries an 8-byte `ColorF16`;
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

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CmdKind {
    /// Scissor clip with optional rounded-corner stencil mask. Carries
    /// [`PushClipPayload`] (rect + corners). When `corners` is all-zero
    /// the composer treats it as a plain scissor; otherwise the
    /// backend's stencil path writes the SDF mask using the radii.
    PushClip,
    PopClip,
    PushTransform,
    PopTransform,
    DrawRect,
    /// Drop / inset box-shadow. Payload: [`DrawShadowPayload`]. Same
    /// `Quad` shape at the GPU end as `DrawRect`, but the composer
    /// scales `fill_axis` (logical-px shadow params) and skips the
    /// stroke / gradient-atlas code paths.
    DrawShadow,
    DrawText,
    /// Mesh paint cmd. Payload: [`DrawMeshPayload`]. Vertex/index
    /// rows live in `RecordPayloads` and are sliced by the payload's spans.
    DrawMesh,
    /// Stroked polyline paint cmd. Payload:
    /// [`DrawPolylinePayload`]. Point storage lives in `RecordPayloads`,
    /// sliced by the payload's span. Composer transforms + DPI-scales
    /// the points, then emits one `CurveInstance` per kept segment
    /// plus one join-chrome instance per interior joint into
    /// `RenderBuffer.curves` — polylines batch and draw with every
    /// other stroke on the GPU curve pipeline.
    DrawPolyline,
    /// Textured rectangle paint cmd. Payload: [`DrawImagePayload`].
    /// The composer transforms `rect` into physical-px and routes to
    /// the backend's image pipeline, which samples the texture
    /// registered against `handle` in the shared
    /// [`ImageRegistry`](crate::renderer::image_registry::ImageRegistry).
    DrawImage,
    /// Native GPU cubic-bezier curve. Payload: [`DrawCurvePayload`].
    /// The composer transforms the four control points to physical-px,
    /// derives an adaptive sub-instance count from the control-polygon
    /// length, and appends one or more `CurveInstance`s into
    /// `RenderBuffer.curves`. A single `draw` per scissor group covers
    /// every instance in the group's curve [`GroupBatch`].
    ///
    /// [`GroupBatch`]: crate::renderer::render_buffer::batch::GroupBatch
    DrawCurve,
    /// Native GPU circular arc. Payload: [`DrawArcPayload`]. Same
    /// batching as `DrawCurve` — the composer transforms center/radius
    /// to physical-px, derives the sub-instance count from the exact
    /// arc length (`radius · |sweep|`), and appends `CurveInstance`s
    /// with `kind = CURVE_KIND_ARC` into the same `RenderBuffer.curves`
    /// stream.
    DrawArc,
    /// Rounded-triangle SDF. Payload: [`DrawTrianglePayload`]. The composer
    /// transforms the three owner-local corner points to physical px, derives
    /// the covering AABB, and emits one `Quad` with `FillKind::TRIANGLE` —
    /// reusing the shared quad pipeline (the three points + radius pack into
    /// the `Quad`'s `corners` / `fill_axis` lanes; the shader evaluates the
    /// triangle SDF per fragment).
    DrawTriangle,
}

/// Scissor clip payload. `corners` is all-zero for plain rect clips
/// and non-zero for rounded-mask clips — the composer decides which
/// path to take by inspecting it.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct PushClipPayload {
    pub(crate) rect: Rect,
    pub(crate) corners: Corners,
}

#[padding_struct::padding_struct]
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct PushTransformPayload {
    pub(crate) translation: glam::Vec2,
    pub(crate) scale: f32,
}

impl From<TranslateScale> for PushTransformPayload {
    fn from(transform: TranslateScale) -> Self {
        Self {
            translation: transform.translation,
            scale: transform.scale,
            ..bytemuck::Zeroable::zeroed()
        }
    }
}

impl From<PushTransformPayload> for TranslateScale {
    fn from(payload: PushTransformPayload) -> Self {
        Self::new(payload.translation, payload.scale)
    }
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
/// write time. `Pod` invariant: `repr(C)` + no padding.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
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
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
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

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct DrawTextPayload {
    pub(crate) rect: Rect,
    pub(crate) color: ColorF16,
    pub(crate) key: TextCacheKey,
}

impl DrawTextPayload {
    /// Canonical noop predicate for this payload — zero-extent rect
    /// or fully transparent color. See `cmd_buffer` module docs for
    /// the noop policy.
    #[inline]
    pub(crate) fn is_noop(&self) -> bool {
        self.rect.is_paint_empty() || self.color.is_noop()
    }
}

/// Stroked polyline payload. `width` is logical px. Points + colors
/// live in the window's [`RecordPayloads`] (`polyline_points` /
/// `polyline_colors`) — the cmd buffer only carries the spans.
/// `colors_len` is 1 (broadcast), `points_len` (per-point), or
/// `points_len - 1` (per-segment), selected by `color_mode`.
///
/// Points are stored **owner-local**; the composer applies `origin`
/// (the owner-rect top-left) before the active push-transform stack.
/// `bbox` is in the same owner-local space.
///
/// [`RecordPayloads`]: crate::record_store::RecordPayloads
#[padding_struct::padding_struct]
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct DrawPolylinePayload {
    pub(crate) bbox: Rect,
    pub(crate) origin: glam::Vec2,
    pub(crate) width: f32,
    /// Paint-time rotation (radians) about `bbox.center()` (owner-local),
    /// applied to each point before the ancestor transform. `0.0` = none,
    /// the common case. Set from a [`PaintAnim::Spin`] sample; the
    /// encoder widens `bbox` to the rotation-invariant owner box so the
    /// scissor cull stays correct and its centre is the spin pivot.
    ///
    /// [`PaintAnim::Spin`]: crate::forest::tree::paint_anims::PaintAnim::Spin
    pub(crate) rotation: f32,
    pub(crate) points_start: u32,
    pub(crate) points_len: u32,
    pub(crate) colors_start: u32,
    pub(crate) colors_len: u32,
    pub(crate) color_mode: ColorModeBits,
    pub(crate) cap: LineCapBits,
    pub(crate) join: LineJoinBits,
}

impl DrawPolylinePayload {
    /// Canonical noop predicate — fewer than two points (no
    /// segments) or zero/negative stroke width. **Does not** check
    /// color noop-ness: per-point / per-segment colours live in
    /// spans on `RenderCmdBuffer`, and an O(n) read here would
    /// dominate the per-cmd cost. Color noop is filtered at the
    /// `Shape::Polyline::is_noop` authoring boundary instead. The
    /// bbox can legitimately be zero-area (horizontal / vertical
    /// line) and still paint stroke pixels, so it's not gated either.
    #[inline]
    pub(crate) fn is_noop(&self) -> bool {
        self.points_len < 2 || self.width <= 0.0
    }
}

/// Mesh draw payload. Vertex/index data lives in the window's
/// [`RecordPayloads`] (`meshes`); the cmd buffer only carries the spans
/// (owner-local). The composer folds `origin` (owner-rect top-left)
/// into the per-instance translate so the vertex stream stays
/// content-stable across frames.
///
/// [`RecordPayloads`]: crate::record_store::RecordPayloads
#[padding_struct::padding_struct]
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
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
/// [`ImageHandle`] — the backend looks it up against its GPU texture
/// cache.
#[padding_struct::padding_struct]
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
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
    /// `IMG_FLAG_*` bits (tile wrap, nearest sampling), forwarded
    /// verbatim into [`ImageInstance::flags`](crate::renderer::render_buffer::image::ImageInstance).
    /// `0` (the common case, including a `GpuView`) samples the UV
    /// directly with the bilinear sampler.
    pub(crate) flags: u32,
    /// An `Option<u32>` `GpuView` link, packed into this `Pod` field (`Option`
    /// itself isn't `Pod`) as the `+1` niche: `0` = ordinary image (so the
    /// `Zeroable` default reads as `None`), `n > 0` = a `GpuView` whose
    /// `GpuPaintRef` is `gpu_view_paints[n - 1]`. Private — set only through
    /// [`Self::image`] / [`Self::gpu_view`], read only through
    /// [`Self::gpu_view_paint`], so the niche never leaks. `handle` carries
    /// the view's stable `TextureId` either way, so the draw + cache path
    /// stays identical to an image.
    target: u32,
}

impl DrawImagePayload {
    /// An ordinary image draw — no off-screen target (`target` is `None`).
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
            target: 0,
            ..bytemuck::Zeroable::zeroed()
        }
    }

    /// A `GpuView` composite over its full arranged `rect`: full UV, untinted,
    /// sampling the view's stable `handle`. `paint_index` (into the cmd
    /// buffer's `gpu_view_paints`) packs into the `target` niche — the sole
    /// `+1`, so the composer can list the off-screen target to paint.
    #[inline]
    pub(crate) fn gpu_view(rect: Rect, handle: TextureId, paint_index: u32) -> Self {
        Self {
            rect,
            uv_min: glam::Vec2::ZERO,
            uv_size: glam::Vec2::ONE,
            tint: ColorF16::from(Color::WHITE),
            handle,
            flags: 0,
            target: paint_index + 1,
            ..bytemuck::Zeroable::zeroed()
        }
    }

    /// The `GpuView` paint index this draw composites, or `None` for an
    /// ordinary image — unpacks the `target` niche.
    #[inline]
    pub(crate) fn gpu_view_paint(&self) -> Option<u32> {
        self.target.checked_sub(1)
    }

    /// Canonical noop predicate — zero-extent rect, fully transparent tint,
    /// or null handle (paints no pixels, no texture to sample). A `GpuView`
    /// (`gpu_view_paint().is_some()`) is never null-skipped — its texture is
    /// framework-painted this frame, not a registered image that could have
    /// been dropped.
    #[inline]
    pub(crate) fn is_noop(&self) -> bool {
        self.rect.is_paint_empty()
            || self.tint.is_noop()
            || (self.handle.0 == 0 && self.gpu_view_paint().is_none())
    }
}

/// Native GPU bezier-curve payload. Four cubic control points are
/// stored owner-local; the composer adds `origin` and the active
/// push-transform stack before scaling to physical px and pushing the
/// resulting `CurveInstance`(s) onto `RenderBuffer.curves`. `bbox` is
/// the owner-local stroked-AABB (already inflated by `width/2 + AA
/// fringe` at lowering) used for clip culling and paint-order overlap.
/// `rotation` is the paint-time spin angle sampled from
/// `PaintAnim::Spin` — non-zero only when the encoder replaced `bbox`
/// with the rotation-invariant square whose centre is the spin pivot
/// (same contract as `DrawPolylinePayload`); the composer rotates the
/// control points about that pivot, which is exact for a bezier
/// (affine invariance).
#[padding_struct::padding_struct]
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct DrawCurvePayload {
    pub(crate) bbox: Rect,
    pub(crate) origin: glam::Vec2,
    pub(crate) rotation: f32,
    pub(crate) p0: glam::Vec2,
    pub(crate) p1: glam::Vec2,
    pub(crate) p2: glam::Vec2,
    pub(crate) p3: glam::Vec2,
    /// Solid stroke colour. Zeroed when `fill_kind` is a gradient —
    /// the LUT row at `fill_lut_row` supplies the colour in that case.
    pub(crate) color: ColorF16,
    pub(crate) width: f32,
    /// Cap kind packed as `u32` (Pod-safe; the variant tag from
    /// `LineCap as u8` widened). Composer threads it into the
    /// `CurveInstance.cap` lane verbatim.
    pub(crate) cap: u32,
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
#[padding_struct::padding_struct]
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
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
    /// Canonical noop predicate — zero/negative stroke width or a
    /// solid fill that's fully transparent. Gradient fills always
    /// paint (the all-transparent-stops case is caught by
    /// `Brush::is_noop` before lowering).
    #[inline]
    pub(crate) fn is_noop(&self) -> bool {
        if noop_f32(self.width) {
            return true;
        }
        self.fill_kind == FillKind::SOLID && self.color.is_noop()
    }
}

/// Native GPU circular-arc payload — the exact-circle sibling of
/// [`DrawCurvePayload`]. `center` is owner-local (composer adds
/// `origin` + the active transform before scaling to physical px);
/// `a0`/`a1` are start/end angles in radians (screen convention,
/// `a1 < a0` for a negative sweep). `rotation` is the paint-time spin
/// angle sampled from `PaintAnim::Spin` — non-zero only when the
/// encoder replaced `bbox` with the rotation-invariant square whose
/// centre is the spin pivot (same contract as `DrawPolylinePayload`).
#[padding_struct::padding_struct]
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct DrawArcPayload {
    pub(crate) bbox: Rect,
    pub(crate) origin: glam::Vec2,
    pub(crate) center: glam::Vec2,
    pub(crate) radius: f32,
    pub(crate) a0: f32,
    pub(crate) a1: f32,
    pub(crate) rotation: f32,
    /// Solid stroke colour; zeroed for gradients (see
    /// [`DrawCurvePayload::color`]).
    pub(crate) color: ColorF16,
    pub(crate) width: f32,
    /// Cap kind packed as `u32` — see [`DrawCurvePayload::cap`].
    pub(crate) cap: u32,
    pub(crate) fill_kind: FillKind,
    pub(crate) fill_lut_row: LutRow,
}

impl DrawArcPayload {
    /// Same predicate as [`DrawCurvePayload::is_noop`], plus a
    /// degenerate radius (nothing to trace).
    #[inline]
    pub(crate) fn is_noop(&self) -> bool {
        if noop_f32(self.width) || noop_f32(self.radius) {
            return true;
        }
        self.fill_kind == FillKind::SOLID && self.color.is_noop()
    }
}
