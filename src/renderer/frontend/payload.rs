//! Lowered paint payloads — the values the encoder hands a
//! [`PaintSink`](crate::renderer::frontend::paint_sink::PaintSink),
//! one per paint operation.
//!
//! Plain value types. Nothing serializes them — the sink consumes each
//! payload inline — so the layout is the compiler's to choose, and
//! fields are ordinary enums rather than the `u8` newtypes,
//! `#[repr(C)]`, and injected trailing padding a `bytemuck::Pod` command
//! arena would require.
//!
//! ## Stroked-shape bounds
//!
//! `DrawPolylinePayload` and `DrawCurvePayload` carry their cull bound
//! and their spin as one [`StrokeBounds`] value rather than a `bbox`
//! plus a `rotation`. The two used to be separate fields whose meaning
//! depended on each other — a non-zero `rotation` silently redefined
//! `bbox` from "centerline AABB" to "the square whose centre is the
//! pivot" — which took a section of prose here, a producer doc, a
//! consumer doc and a `debug_assert` to hold together. The enum says it
//! instead: you cannot read an AABB off a spun shape, and you cannot
//! get a pivot without one.

use crate::icons::icon_set::IconRef;
use crate::primitives::approx::noop_f32;
use crate::primitives::brush::gradient::FillAxis;
use crate::primitives::fill_wire::{FillKind, LutRow};
use crate::primitives::size::Size;
use crate::primitives::texture_id::TextureId;
use crate::primitives::{color::ColorF16, corners::Corners, rect::Rect};
use crate::scene::shapes::paint::{CurveBasis, ShapeStroke};
use crate::scene::shapes::record::ColorMode;
use crate::shape::style::{LineCap, LineJoin};
use crate::text::shaped_ref::ShapedTextRef;
use glam::Vec2;

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

impl PushClipPayload {
    /// A plain rect clip — zero corners, which is what tells the
    /// composer to take the scissor path rather than the rounded mask.
    pub(crate) fn rect(rect: Rect) -> Self {
        Self {
            rect,
            corners: Corners::ZERO,
        }
    }

    pub(crate) fn rounded(rect: Rect, corners: Corners) -> Self {
        Self { rect, corners }
    }
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
    /// A rounded rect with `fill` and `stroke`.
    ///
    /// The brush is lowered here rather than at the call site because
    /// the GPU lanes it fills — colour, kind, LUT row, axis — are this
    /// type's, and a caller assembling them by hand is a caller that
    /// can get the gradient case wrong.
    pub(crate) fn rect(
        rect: Rect,
        corners: Corners,
        fill: BrushSource,
        stroke: ShapeStroke,
    ) -> Self {
        Self::rect_impl(rect, corners, fill, stroke, false)
    }

    /// Windowed sibling of [`Self::rect`]: same payload, but the
    /// `FillKind` carries the window bit, so the shader inverts the fill
    /// coverage (fill outside the rounded boundary, transparent window
    /// inside the stroke). The bit also keeps the composer's opaque-cover
    /// checks (`fill_kind == FillKind::SOLID`) from treating the quad as
    /// an occluder — its interior is a hole.
    pub(crate) fn rect_window(
        rect: Rect,
        corners: Corners,
        fill: BrushSource,
        stroke: ShapeStroke,
    ) -> Self {
        Self::rect_impl(rect, corners, fill, stroke, true)
    }

    fn rect_impl(
        rect: Rect,
        corners: Corners,
        fill: BrushSource,
        stroke: ShapeStroke,
        window: bool,
    ) -> Self {
        // Stroke stays solid-only — gradient strokes are a non-goal.
        let GpuFillFields {
            color: fill_color,
            kind: fill_kind,
            lut_row: fill_lut_row,
            axis: fill_axis,
        } = fill.to_gpu_fields();
        Self {
            geom: QuadGeom::Rect { rect, corners },
            fill: fill_color,
            stroke: stroke.normalized(),
            fill_kind: if window {
                fill_kind.with_window()
            } else {
                fill_kind
            },
            fill_lut_row,
            fill_axis,
        }
    }

    /// A shadow. For a drop shadow, `rect` is the offset source inflated
    /// by `3σ + max(spread, 0)`; for an inset shadow it is the source
    /// rect. `corners` is the source shape's corner radii, `color` the
    /// shadow tint, `fill_kind` `FillKind::SHADOW_DROP|SHADOW_INSET`.
    /// Drop shadows carry `(0, 0, σ, spread)` in `fill_axis`; inset
    /// shadows carry `(offset.x, offset.y, σ, spread)`. The composer
    /// scales the logical-px lanes to physical px on emit.
    pub(crate) fn shadow(
        rect: Rect,
        corners: Corners,
        color: ColorF16,
        fill_kind: FillKind,
        fill_axis: FillAxis,
    ) -> Self {
        Self {
            geom: QuadGeom::Rect { rect, corners },
            fill: color,
            // A shadow has no stroke; its whole edge is the blur.
            stroke: ShapeStroke::NONE,
            fill_kind,
            fill_lut_row: LutRow::FALLBACK,
            fill_axis,
        }
    }

    /// A rounded triangle from three owner-local `points` offset by
    /// `origin`. Same quad tier as [`Self::rect`], down to the shared
    /// stroke normalization — only the geometry and the SDF the
    /// `FillKind` selects differ.
    pub(crate) fn triangle(
        origin: glam::Vec2,
        points: [glam::Vec2; 3],
        fill: ColorF16,
        radius: f32,
        stroke: ShapeStroke,
    ) -> Self {
        let [a, b, c] = points;
        Self {
            geom: QuadGeom::Triangle {
                origin,
                a,
                b,
                c,
                radius,
            },
            fill,
            stroke: stroke.normalized(),
            fill_kind: FillKind::TRIANGLE,
            // The composer overwrites both reused lanes from the
            // transformed points, so neither is read from here.
            fill_lut_row: LutRow::FALLBACK,
            fill_axis: FillAxis::ZERO,
        }
    }

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
/// Where a stroked shape rotates, for the shapes that do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Spin {
    /// Owner-local point the shape turns about.
    pub(crate) pivot: Vec2,
    /// Radians, applied to each point before the ancestor transform.
    pub(crate) angle: f32,
}

/// A stroked shape's owner-local cull bound, and its spin if it has one.
///
/// One value rather than a `bbox: Rect` beside a `rotation: f32`,
/// because the two were not independent: a non-zero rotation meant the
/// rect was no longer the centerline AABB but the rotation-invariant
/// square about the pivot, and `bbox.center()` was the only way the
/// composer could recover that pivot once the owner rect was gone.
/// Encoding it here means the still case cannot carry a stale pivot and
/// the spun case cannot lose one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum StrokeBounds {
    /// Centerline AABB, owner-local.
    Still(Rect),
    /// A spun shape sweeps a disc about `spin.pivot`, so what it is
    /// culled and batched against is that disc's bounding square —
    /// rotation-invariant, which is what keeps the composer's overlap
    /// tracking correct at every angle. Stroke reach is applied after
    /// it, in physical space.
    Spun { spin: Spin, radius: f32 },
}

impl Default for StrokeBounds {
    fn default() -> Self {
        Self::Still(Rect::default())
    }
}

impl StrokeBounds {
    /// Owner-local rect the composer culls and batches against.
    #[inline]
    pub(crate) fn cull_rect(self) -> Rect {
        match self {
            Self::Still(bbox) => bbox,
            Self::Spun { spin, radius } => Rect {
                min: spin.pivot - Vec2::splat(radius),
                size: Size {
                    w: 2.0 * radius,
                    h: 2.0 * radius,
                },
            },
        }
    }

    /// The spin to draw under, or `None` for the common still case.
    #[inline]
    pub(crate) fn spin(self) -> Option<Spin> {
        match self {
            Self::Still(_) => None,
            Self::Spun { spin, .. } => Some(spin),
        }
    }
}

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
    /// Cull bound plus the spin, if any — set from a
    /// [`PaintAnim::Spin`] sample.
    ///
    /// [`PaintAnim::Spin`]: crate::scene::tree::paint_anims::PaintAnim::Spin
    pub(crate) bounds: StrokeBounds,
    pub(crate) origin: glam::Vec2,
    pub(crate) width: f32,
    pub(crate) points_start: u32,
    pub(crate) points_len: u32,
    pub(crate) colors_start: u32,
    pub(crate) colors_len: u32,
    pub(crate) color_mode: ColorMode,
    pub(crate) cap: LineCap,
    pub(crate) join: LineJoin,
}

impl DrawPolylinePayload {
    /// Paints nothing when: fewer than two points (no segments) or a
    /// non-paintable stroke width.
    ///
    /// Unlike its siblings this is an **invariant**, not a filter —
    /// `PaintSink::draw_polyline` asserts it rather than gating on it,
    /// because both conditions are authoring-derived and already
    /// guaranteed by `Shape::Polyline::is_noop`. See that method for
    /// why the two differ.
    ///
    /// **Does not** check colour noop-ness: per-point / per-segment
    /// colours live in spans on the record store, and an O(n) read here
    /// would dominate the per-call cost. Colour noop is filtered at the
    /// `Shape::Polyline::is_noop` authoring boundary instead. The bbox
    /// can legitimately be zero-area (horizontal / vertical line) and
    /// still paint stroke pixels, so it isn't checked either.
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
    /// `IMG_FLAG_*` bits (tile wrap, min/mag nearest sampling, minification
    /// tap mode), forwarded
    /// verbatim into [`ImageInstance::flags`](crate::renderer::render_buffer::image::ImageInstance).
    /// `0` (the common case, including a `GpuView`) takes one bilinear tap at
    /// the UV.
    pub(crate) flags: u32,
}

/// One baked-icon draw, in logical px.
///
/// Deliberately small and deliberately unresolved: the physical box, the
/// raster size, and the atlas slot are all decided downstream, because none of
/// them are known until the display scale and ancestor transforms have been
/// applied. What the encoder settles is which icon, where, and in what colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawIconPayload {
    /// Fit-resolved paint rect in logical px.
    pub(crate) rect: Rect,
    pub(crate) icon: IconRef,
    /// Whole tint for a tintable icon, alpha only for a colour one.
    pub(crate) tint: ColorF16,
    /// Draw a colour icon as its own luminance.
    pub(crate) desaturate: bool,
}

impl DrawIconPayload {
    /// Paints nothing when the rect has no extent or the tint is fully
    /// transparent — the latter covering both icon kinds, since alpha gates
    /// the colour path as much as the mask one.
    #[inline]
    pub(crate) fn is_noop(&self) -> bool {
        self.rect.is_paint_empty() || self.tint.is_noop()
    }
}

impl DrawImagePayload {
    /// An image draw.
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
        }
    }

    /// Paints nothing when: zero-extent rect, fully transparent tint,
    /// or null handle (paints no pixels, no texture to sample).
    ///
    /// `is_gpu_view` rather than a field, because it is the same fact as
    /// the `paint` callback the sink already carries beside the payload —
    /// a `GpuView` is never null-skipped, since its texture is
    /// framework-painted this frame rather than a registered image that
    /// could have been dropped. Held on the payload it would ride in
    /// every `PartialEq` and every captured call for one read, two lines
    /// after the write.
    #[inline]
    pub(crate) fn is_noop(&self, is_gpu_view: bool) -> bool {
        self.rect.is_paint_empty() || self.tint.is_noop() || (self.handle.0 == 0 && !is_gpu_view)
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
    /// Cull bound plus the spin, if any. The composer rotates about the
    /// pivot exactly — a Bézier by affine invariance, a circle by moving
    /// its centre and shifting both angles.
    pub(crate) bounds: StrokeBounds,
    pub(crate) origin: glam::Vec2,
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

#[cfg(test)]
mod tests {
    use crate::primitives::color::{Color, ColorF16};
    use crate::primitives::corners::Corners;
    use crate::primitives::fill_wire::FillKind;
    use crate::primitives::rect::Rect;
    use crate::primitives::stroke::Stroke;
    use crate::renderer::frontend::payload::{BrushSource, DrawQuadPayload, QuadGeom};
    use crate::scene::shapes::paint::ShapeStroke;
    use glam::Vec2;

    /// Every quad-tier constructor runs one stroke normalization
    /// ([`ShapeStroke::normalized`]): a noop stroke — transparent
    /// colour, zero width, or a NaN width — lands in the payload as
    /// [`ShapeStroke::NONE`]; anything else passes through verbatim.
    ///
    /// The NaN row is the interesting one. It normalizes away like any
    /// other non-painting width, which is deliberate: catching a NaN
    /// *loudly* is `Shape::debug_assert_no_nan`'s job at the authoring
    /// boundary, so by the time a value reaches here the useful
    /// behaviour is to fail safe — and to do it identically for every
    /// shape, rather than per-path (a rect forwarding NaN to the GPU
    /// while a triangle scrubs it).
    ///
    /// The table pins both halves: the exact normalized stroke per case,
    /// **and** that [`DrawQuadPayload::rect`] and
    /// [`DrawQuadPayload::triangle`] produce bit-identical stroke fields
    /// — the regression guard against either growing its own copy again.
    #[test]
    fn quad_stroke_normalization_is_shared_by_rect_and_triangle() {
        let fill = Color::rgb(1.0, 0.0, 0.0);
        let green = Color::rgb(0.0, 1.0, 0.0);
        let cases: [(&str, ShapeStroke, bool); 4] = [
            (
                "transparent_color",
                Stroke::solid(Color::TRANSPARENT, 3.0).into(),
                true,
            ),
            ("zero_width", Stroke::solid(green, 0.0).into(), true),
            ("nan_width", Stroke::solid(green, f32::NAN).into(), true),
            ("live", Stroke::solid(green, 3.0).into(), false),
        ];
        for (label, stroke, expect_normalized) in cases {
            let rp = DrawQuadPayload::rect(
                Rect::new(0.0, 0.0, 10.0, 10.0),
                Corners::ZERO,
                BrushSource::Solid(fill.into()),
                stroke,
            );
            assert!(
                matches!(rp.geom, QuadGeom::Rect { .. }),
                "case {label}: rect must carry rect geometry",
            );

            let tp = DrawQuadPayload::triangle(
                Vec2::ZERO,
                [
                    Vec2::new(0.0, 0.0),
                    Vec2::new(10.0, 0.0),
                    Vec2::new(5.0, 8.0),
                ],
                fill.into(),
                0.0,
                stroke,
            );
            assert!(
                matches!(tp.geom, QuadGeom::Triangle { .. }),
                "case {label}: triangle must carry triangle geometry",
            );
            assert_eq!(tp.fill_kind, FillKind::TRIANGLE, "case {label}");

            assert_eq!(tp.stroke.color, rp.stroke.color, "case {label}");
            assert_eq!(
                tp.stroke.width.to_bits(),
                rp.stroke.width.to_bits(),
                "case {label}",
            );
            if expect_normalized {
                assert_eq!(tp.stroke.color, ColorF16::TRANSPARENT, "case {label}");
                assert_eq!(tp.stroke.width, 0.0, "case {label}");
            } else {
                assert_eq!(tp.stroke.color, ColorF16::from(green), "case {label}");
            }
        }
    }
}
