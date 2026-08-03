//! Lowered paint payloads — the values the encoder hands a
//! [`PaintSink`](crate::renderer::frontend::paint_sink::PaintSink),
//! one per paint operation.
//!
//! Plain value types: they used to be `bytemuck::Pod` so a packed
//! command arena could store them, and carried `#[repr(C)]` plus
//! injected trailing padding to satisfy that. Nothing serializes them
//! now, so the layout is the compiler's to choose and fields can be
//! ordinary enums rather than `u8` newtypes.
//!
//! ## The spin pivot contract
//!
//! Stated once here because it binds two payload fields across three
//! tiers, and the three used to restate it separately.
//!
//! `DrawPolylinePayload` and `DrawCurvePayload` each carry a `bbox`
//! and a `rotation`. **Whenever `rotation != 0`, `bbox` is not the
//! shape's centerline AABB** — the encoder's `spin_bbox` has replaced
//! it with the smallest square centred on the owner-box centre that
//! still contains that AABB. Two things follow, and both are relied on:
//!
//! - The square is rotation-invariant, so the composer's cull and
//!   overlap tracking stay correct at every angle. Stroke reach is
//!   applied after it, in physical space.
//! - `bbox.center()` **is** the spin pivot, by construction. It is the
//!   only way the composer can recover the pivot: the owner rect is
//!   long gone by then.
//!
//! Producer: `spin_bbox` (encoder), covered by
//! `encoder::tests::spun_*_bbox_is_rotation_invariant_square_about_owner_centre`.
//! Consumer: `spin_pivot` (composer), which debug-asserts the square so
//! a future emit path that skips `spin_bbox` trips instead of spinning
//! about the wrong point.

use crate::primitives::approx::noop_f32;
use crate::primitives::brush::gradient::FillAxis;
use crate::primitives::fill_wire::{FillKind, LutRow};
use crate::primitives::texture_id::TextureId;
use crate::primitives::{color::ColorF16, corners::Corners, rect::Rect};
use crate::scene::shapes::paint::{CurveBasis, ShapeStroke};
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

/// The geometry half of a [`DrawQuadPayload`] — everything the composer
/// needs to derive the instance's physical rect and its two reused
/// lanes. Rectangles, windowed rectangles, and box-shadows all arrive
/// as an already-resolved rect, so they share one variant; a triangle
/// is the one shape whose covering rect only exists *after* its points
/// are transformed, so it carries the points instead.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum QuadGeom {
    /// A logical-px paint rect + corner radii. For a drop shadow the
    /// rect is the offset source inflated by `3σ + max(spread, 0)`; for
    /// an inset shadow it is the source, and `corners` carries the
    /// *source* shape's radii either way.
    Rect { rect: Rect, corners: Corners },
    /// Owner-local corner points and corner rounding. The composer
    /// folds `origin` (the owner-rect top-left) + the active
    /// push-transform before scaling to physical px, then derives the
    /// covering AABB (the points inflated by `radius + AA fringe`) and
    /// packs the physical points into the `corners` / `fill_axis` lanes.
    Triangle {
        origin: glam::Vec2,
        a: glam::Vec2,
        b: glam::Vec2,
        c: glam::Vec2,
        radius: f32,
    },
}

impl QuadGeom {
    /// Whether this geometry covers no pixels on its own. A triangle
    /// always answers `false`: its covering rect doesn't exist until the
    /// composer transforms the points, and degenerate corners are
    /// already filtered at the authoring boundary by
    /// `TriangleShape::is_noop`.
    #[inline]
    fn is_paint_empty(&self) -> bool {
        match self {
            Self::Rect { rect, .. } => rect.is_paint_empty(),
            Self::Triangle { .. } => false,
        }
    }
}

/// One quad-tier draw: a rounded rect, a windowed rect, a box-shadow,
/// or a rounded triangle. All four lower to a single `Quad` instance on
/// the one quad pipeline, so they share this payload and differ only in
/// [`geom`](Self::geom) and which SDF `fill_kind` selects.
///
/// `fill_kind`'s low byte is the kind tag; bits 8..16 carry `Spread` for
/// gradient variants. `fill_lut_row` is the pre-registered gradient
/// atlas row (set at shape lowering time), or [`LutRow::FALLBACK`] for
/// everything else. `fill_axis` carries gradient geometry packed at
/// lowering, or — for a shadow — `(0, 0, σ, spread)` for drops and
/// `(offset.x, offset.y, σ, spread)` for insets in logical px, which the
/// composer scales to physical px so the shader's `local` coords line
/// up. A triangle's `fill_axis` is unread; the composer overwrites both
/// reused lanes from the transformed points.
///
/// `fill: ColorF16` is the solid colour when `kind == SOLID` (and the
/// tint when it's a shadow); zeroed for gradients, where the atlas row
/// supplies the colour. Storing as `ColorF16` (8 B linear-RGB) vs. 16 B
/// `Color` saves 8 B per payload — the composer decodes via
/// `Color::from(f16)` at `Quad` write time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawQuadPayload {
    pub(crate) geom: QuadGeom,
    /// Linear-RGB fill (straight alpha). Zeroed for gradients; the
    /// atlas row at `fill_lut_row` supplies the colour in that case.
    pub(crate) fill: ColorF16,
    /// Normalized by [`ShapeStroke::normalized`] on the way in, so
    /// [`ShapeStroke::NONE`] here means "no stroke" exactly.
    pub(crate) stroke: ShapeStroke,
    pub(crate) fill_kind: FillKind,
    pub(crate) fill_lut_row: LutRow,
    pub(crate) fill_axis: FillAxis,
}

impl DrawQuadPayload {
    /// Paints nothing when the geometry covers no pixels, or when
    /// neither the fill nor the stroke can put down a texel.
    ///
    /// Shadow parameters themselves (`fill_axis`) are not gated: a
    /// zero-σ drop shadow still paints a hard-edged shifted rect, and
    /// the `Shape::Shadow::is_noop` authoring boundary is what catches
    /// the "no visible effect" cases.
    #[inline]
    pub(crate) fn is_noop(&self) -> bool {
        self.geom.is_paint_empty() || (self.fill_is_noop() && self.stroke.is_noop())
    }

    /// A gradient's colour lane is zeroed by
    /// [`BrushSource::to_gpu_fields`] (the atlas row supplies the
    /// colour), so only a non-gradient fill can be judged by its
    /// colour — testing it blind would read every gradient as
    /// transparent and drop the draw.
    ///
    /// A gradient is therefore never a no-op here. It doesn't need to
    /// be: the all-transparent-stops case is filtered by
    /// `Brush::is_noop` *before* lowering, and one slipping past that
    /// gate would paint a useless transparent quad whose alpha blend
    /// produces nothing visible, so correctness is intact either way.
    #[inline]
    fn fill_is_noop(&self) -> bool {
        !self.fill_kind.is_gradient() && self.fill.is_noop()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawTextPayload {
    pub(crate) rect: Rect,
    pub(crate) color: ColorF16,
    pub(crate) text: ShapedTextRef,
}

impl DrawTextPayload {
    /// Paints nothing when: zero-extent rect
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
    /// Paint-time rotation (radians) about `bbox.center()`, applied to
    /// each point before the ancestor transform. `0.0` = none, the
    /// common case. Set from a [`PaintAnim::Spin`] sample. Non-zero
    /// means `bbox` is the widened square the pivot contract describes
    /// — see the module doc.
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
    /// Paints nothing when: fewer than two points (no
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
    /// Paints nothing when: empty vertex buffer, fewer than
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
    /// rather than a registered image. **Written in exactly one place** —
    /// [`PaintSink::draw_image`], from `paint.is_some()` — so the flag
    /// cannot disagree with whether a paint callback actually rode
    /// along. It only tells [`Self::is_noop`] not to null-skip a
    /// framework-painted texture; `handle` carries the view's stable
    /// `TextureId` either way, so the draw + cache path stays identical
    /// to an image.
    ///
    /// [`PaintSink::draw_image`]: crate::renderer::frontend::paint_sink::PaintSink::draw_image
    pub(crate) gpu_view: bool,
}

impl DrawImagePayload {
    /// An image draw. `gpu_view` starts `false` and is decided by the
    /// [`PaintSink`](crate::renderer::frontend::paint_sink::PaintSink)
    /// gate; callers pass the paint callback there rather than flagging
    /// the payload themselves.
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

    /// Paints nothing when: zero-extent rect, fully transparent tint,
    /// or null handle (paints no pixels, no texture to sample). A
    /// `GpuView` is never null-skipped — its texture is framework-painted
    /// this frame, not a registered image that could have been dropped.
    #[inline]
    pub(crate) fn is_noop(&self) -> bool {
        self.rect.is_paint_empty() || self.tint.is_noop() || (self.handle.0 == 0 && !self.gpu_view)
    }
}

/// Native GPU stroke payload — a cubic or an arc, per [`CurveBasis`].
/// The composer adds `origin` and the active push-transform stack
/// before scaling to physical px and pushing the resulting
/// `CurveInstance`(s) onto `RenderBuffer.curves`. `bbox` is the
/// owner-local centerline AABB; the composer applies the shared
/// stroke/cap/AA bound in physical space for culling and overlap.
/// `rotation` carries the spin angle under the pivot contract in the
/// module doc; the composer rotates about that pivot exactly — a Bézier
/// by affine invariance, a circle by moving its centre and shifting
/// both angles.
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

impl DrawCurvePayload {
    /// Paints nothing when: zero/negative stroke width, a
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
