//! `RenderCmdBuffer` — SoA command stream.
//!
//! Three columns: a 1-byte kind discriminant per command, a `u32`
//! start offset into a payload arena, and the arena itself. Consumers
//! walk `kinds` / `starts` by index and read each payload with the
//! typed `read::<T>()` helper — no command-enum is ever materialized.
//! All variants are paint ops the composer scales, snaps, and groups
//! into the `RenderBuffer`.
//!
//! Memory: a tagged-enum representation would size to its largest
//! variant (~80 B with padding), so a sequence of
//! `PopClip`/`PopTransform` would pay full-variant storage. Here Pops
//! are 1 + 4 = 5 bytes (kind byte + start offset, no payload).
//! `DrawRect` splits into stroked / unstroked kinds so the no-stroke
//! variant skips the 5×u32 stroke payload entirely.
//!
//! Soundness: payload structs are `#[repr(C)]` aggregates of
//! `f32`/`u32` (and one `u64` in `TextCacheKey`) tagged
//! `bytemuck::Pod`, so the compiler proves they have no padding bytes.
//! The arena is `Vec<u32>` (4-byte aligned). Pushes go through
//! `bytemuck::cast_slice` (safe); reads go through
//! `bytemuck::pod_read_unaligned` so payloads with align >4
//! (`DrawTextPayload`) work even when the arena slot starts at a
//! 4-byte-only-aligned offset.
//!
//! ## Noop policy
//!
//! Every `draw_*` early-returns when its inputs would emit no visible
//! pixels (transparent fill color, no-op stroke, no-op shadow tint).
//! **The cmd buffer is the single canonical correctness gate** —
//! callers don't need to pre-check, and the encoder doesn't gate per
//! branch. Upstream filters (`Shape::is_noop` at `Ui::add_shape`,
//! whole-`Background::is_noop` at `Tree::open_node`) are performance
//! optimizations that skip expensive lowering (text shaping, polyline
//! tessellation) or sparse-column writes, not correctness gates.
//!
//! Exception: `draw_polyline` doesn't gate. Its colors live in spans
//! (`PerSegment` can mix one solid stop with N transparent), and an
//! O(n) read on every emit would dominate the per-cmd cost. Polyline
//! noops are caught by `Shape::Polyline::is_noop` at the authoring
//! boundary, which is the only practical gate.

use crate::forest::shapes::payloads::ShapePayloads;
use crate::forest::shapes::record::ShapeStroke;
use crate::primitives::brush::{
    Brush, ConicGradient, FillAxis, Interp, LinearGradient, MAX_STOPS, RadialGradient, Stop,
};
use crate::primitives::{color::ColorF16, corners::Corners, rect::Rect, transform::TranslateScale};
use crate::renderer::quad::FillKind;
use crate::shape::{ColorModeBits, LineCapBits, LineJoinBits};
use crate::text::TextCacheKey;
use tinyvec::ArrayVec;

/// Borrow-form brush for the cmd-buffer side: `Solid` inline,
/// gradient variants borrowed from the per-frame `Shapes.gradients`
/// arena (or directly from a user-side `Brush`). Avoids spilling the
/// 88-byte `Brush` enum to the stack on the hot solid path — `Color`
/// inline is 16 B, gradient is an 8-byte pointer, total ~24 B.
#[derive(Clone, Copy, Debug)]
pub(crate) enum BrushSource<'a> {
    Solid(ColorF16),
    Linear(&'a LinearGradient),
    Radial(&'a RadialGradient),
    Conic(&'a ConicGradient),
}

impl BrushSource<'_> {
    #[inline]
    pub(crate) fn is_noop(self) -> bool {
        match self {
            Self::Solid(c) => c.is_noop(),
            Self::Linear(g) => g.is_noop(),
            Self::Radial(g) => g.is_noop(),
            Self::Conic(g) => g.is_noop(),
        }
    }
}

impl<'a> From<&'a Brush> for BrushSource<'a> {
    #[inline]
    fn from(b: &'a Brush) -> Self {
        match b {
            Brush::Solid(c) => Self::Solid((*c).into()),
            Brush::Linear(g) => Self::Linear(g),
            Brush::Radial(g) => Self::Radial(g),
            Brush::Conic(g) => Self::Conic(g),
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CmdKind {
    /// Scissor clip with optional rounded-corner stencil mask. Carries
    /// [`PushClipPayload`] (rect + radius). When `radius` is all-zero
    /// the composer treats it as a plain scissor; otherwise the
    /// backend's stencil path writes the SDF mask using the radius.
    PushClip,
    PopClip,
    PushTransform,
    PopTransform,
    DrawRect,
    DrawText,
    /// Mesh paint cmd. Payload: [`DrawMeshPayload`]. Vertex/index
    /// bytes live in [`RenderCmdBuffer::mesh_vertices`] /
    /// `mesh_indices`, sliced by the payload's spans.
    DrawMesh,
    /// Stroked polyline paint cmd. Payload:
    /// [`DrawPolylinePayload`]. Point arena lives in
    /// [`RenderCmdBuffer::polyline_points`], sliced by the payload's
    /// span. Composer transforms + DPI-scales the points, then
    /// tessellates a fringe-AA stroke into `out.meshes.arena` —
    /// final paint reuses the mesh pipeline.
    DrawPolyline,
}

/// Scissor clip payload. `radius` is all-zero for plain rect clips
/// and non-zero for rounded-mask clips — the composer decides which
/// path to take by inspecting it.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct PushClipPayload {
    pub(crate) rect: Rect,
    pub(crate) radius: Corners,
}

/// Brush metadata packed into draw-rect payloads. `fill_kind` low byte
/// is the kind tag; bits 8..16 carry `Spread` for gradient variants.
/// `fill_grad_idx` indexes into [`RenderCmdBuffer::gradient_lut_keys`]
/// when `fill_kind.is_gradient()`; unused (and unread) for solid.
/// `fill_axis` carries gradient geometry computed at encode time from
/// the brush's `axis()`. `fill: ColorF16` is the solid colour when
/// `kind == SOLID`; for gradients it's zeroed and the composer's
/// atlas lookup supplies the LUT row. Storing as `ColorF16` (4 B per
/// colour vs. 16 B `Color`) saves 24 B per rect payload — the
/// composer decodes via `Color::from(srgb)` at `Quad` write time.
/// `Pod` invariant: `repr(C)` + no padding; the proc macro at the
/// end of the field list backfills if alignment shifts.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct DrawRectPayload {
    pub(crate) rect: Rect,
    pub(crate) radius: Corners,
    /// sRGB-encoded fill. Zeroed for gradients; composer's atlas
    /// lookup supplies the LUT row in that case.
    pub(crate) fill: ColorF16,
    pub(crate) stroke_color: ColorF16,
    pub(crate) stroke_width: f32,
    pub(crate) fill_kind: FillKind,
    pub(crate) fill_grad_idx: u32,
    pub(crate) fill_axis: FillAxis,
}

impl DrawRectPayload {
    /// Post-pack noop predicate — zero-extent rect, or no visible
    /// paint at all (transparent inline `fill` **and** zero-width /
    /// noop stroke). Gradient `fill_kind` always reports painting at
    /// this layer because the inline `fill` is zeroed for gradients
    /// (the atlas LUT row supplies the color); the all-transparent-
    /// stops case is filtered upstream in `draw_rect` via
    /// `Brush::is_noop`, which sees the stops directly.
    #[inline]
    pub(crate) fn is_noop(&self) -> bool {
        if self.rect.is_paint_empty() {
            return true;
        }
        let fill_noop = if self.fill_kind.is_gradient() {
            false
        } else {
            self.fill.is_noop()
        };
        let stroke_noop = self.stroke_width <= 0.0 || self.stroke_color.is_noop();
        fill_noop && stroke_noop
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

/// Stroked polyline payload. `width` is logical px. Points +
/// colors live in [`RenderCmdBuffer::polyline_points`] /
/// [`RenderCmdBuffer::polyline_colors`]; `colors_len` is 1
/// (broadcast), `points_len` (per-point), or `points_len - 1`
/// (per-segment), selected by `color_mode`.
///
/// `bbox` is the axis-aligned bounds of `points` in logical
/// (cmd-buffer) coords. Composer transforms the 4 corners
/// (uniform-scale `TranslateScale` preserves AABBs), inflates by
/// the physical-px outer-fringe offset, and short-circuits the
/// per-point transform when the result misses the active scissor.
///
/// `color_mode` / `cap` / `join` are `u8` storage tags. Trailing
/// padding is injected by [`padding_struct::padding_struct`] so
/// the struct stays a multiple of its alignment without
/// hand-named `_pad` fields rotting when fields shift. Construct
/// with `..bytemuck::Zeroable::zeroed()` to fill whatever padding
/// the macro generated.
#[padding_struct::padding_struct]
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct DrawPolylinePayload {
    pub(crate) bbox: Rect,
    pub(crate) width: f32,
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

/// Mesh draw payload. Spans are inlined as `(start, len)` u32 pairs so
/// the payload is plain Pod — no `Span: Pod` needed. Vertex positions
/// are already in logical-px world-coords (encoder pre-translates by
/// the owner's top-left), matching the polyline convention.
#[padding_struct::padding_struct]
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct DrawMeshPayload {
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

/// Append-only command buffer. See module docs.
#[derive(Default)]
pub(crate) struct RenderCmdBuffer {
    pub(crate) kinds: Vec<CmdKind>,
    pub(crate) starts: Vec<u32>,
    pub(crate) data: Vec<u32>,
    /// Per-variant geometry referenced by `DrawMesh` / `DrawPolyline`
    /// payloads (spans slice into the arenas inside this). The encoder
    /// pre-translates polyline points by their owner's top-left and
    /// stores meshes verbatim, so the data is owner-relative input
    /// the composer can read without needing `&Tree`. Self-containment
    /// pins the composer's contract — `&RenderCmdBuffer → RenderBuffer`
    /// with no recording-state reach-back. See [`ShapePayloads`].
    pub(crate) shape_payloads: ShapePayloads,
    /// Per-frame arena of gradient LUT keys referenced by
    /// `DrawRectPayload::fill_grad_idx`. Composer reads through this
    /// to register the gradient with the LUT atlas and pack the
    /// resulting row id into `Quad`. Variant-agnostic: linear / radial
    /// / conic gradients all push a `GradientLutKey { stops, interp }`
    /// — the per-fragment `t` derivation lives entirely in the shader,
    /// driven by `fill_kind` + `fill_axis`. Cleared every frame;
    /// capacity retained — steady-state alloc-free.
    pub(crate) gradient_lut_keys: Vec<GradientLutKey>,
}

/// Per-frame entry the composer hands to the LUT atlas. Geometry has
/// already been packed into the cmd's `FillAxis`; only the bake inputs
/// survive here.
#[derive(Clone, Debug)]
pub(crate) struct GradientLutKey {
    pub(crate) stops: ArrayVec<[Stop; MAX_STOPS]>,
    pub(crate) interp: Interp,
}

/// Result of lowering a `Brush` into draw-rect payload fields.
struct BrushPack {
    fill_color: ColorF16,
    fill_kind: FillKind,
    fill_grad_idx: u32,
    fill_axis: FillAxis,
}

impl RenderCmdBuffer {
    pub(crate) fn clear(&mut self) {
        self.kinds.clear();
        self.starts.clear();
        self.data.clear();
        self.shape_payloads.clear();
        self.gradient_lut_keys.clear();
    }

    #[inline]
    pub(crate) fn push_clip(&mut self, rect: Rect) {
        self.record_start(CmdKind::PushClip);
        write_pod(
            &mut self.data,
            PushClipPayload {
                rect,
                radius: Corners::ZERO,
            },
        );
    }

    #[inline]
    pub(crate) fn push_clip_rounded(&mut self, rect: Rect, radius: Corners) {
        self.record_start(CmdKind::PushClip);
        write_pod(&mut self.data, PushClipPayload { rect, radius });
    }

    #[inline]
    pub(crate) fn pop_clip(&mut self) {
        self.record_start(CmdKind::PopClip);
    }

    #[inline]
    pub(crate) fn push_transform(&mut self, t: TranslateScale) {
        self.record_start(CmdKind::PushTransform);
        write_pod(&mut self.data, t);
    }

    #[inline]
    pub(crate) fn pop_transform(&mut self) {
        self.record_start(CmdKind::PopTransform);
    }

    #[inline]
    pub(crate) fn draw_rect(
        &mut self,
        rect: Rect,
        radius: Corners,
        fill: BrushSource<'_>,
        stroke: ShapeStroke,
    ) {
        // Pre-pack gate: `Brush::is_noop` peeks inside gradient stops
        // (which the packed payload's inline `fill` no longer carries
        // — gradient color lives in the atlas LUT row indexed by
        // `fill_grad_idx`). Catching the all-transparent-stops case
        // here also avoids the atlas registration and `BrushPack`
        // build downstream.
        if rect.is_paint_empty() || (fill.is_noop() && stroke.is_noop()) {
            return;
        }

        // Stroke stays solid-only — gradient strokes are a non-goal.
        let BrushPack {
            fill_color,
            fill_kind,
            fill_grad_idx,
            fill_axis,
        } = self.pack_brush(fill);

        let (stroke_color, stroke_width) = if stroke.is_noop() {
            (ColorF16::TRANSPARENT, 0.0)
        } else {
            (stroke.color, stroke.width())
        };
        let payload = DrawRectPayload {
            rect,
            radius,
            fill: fill_color,
            stroke_color,
            stroke_width,
            fill_kind,
            fill_grad_idx,
            fill_axis,
        };
        // Defense-in-depth: payload predicate covers post-pack states
        // the pre-pack gate can't (e.g. a stroke that animation-decays
        // between the two checks). Cheap; same noop policy.
        if payload.is_noop() {
            return;
        }
        self.record_start(CmdKind::DrawRect);
        write_pod(&mut self.data, payload);
    }

    /// Record a shadow paint cmd. Reuses the `DrawRect` slot — shadow
    /// is just another quad-kind quad. `rect` is the paint bbox
    /// (source.inflated by `|offset| + 3σ + spread` per axis at
    /// encode time). `radius` is the *source* shape's corner radii.
    /// `color` is the shadow tint. `fill_kind` is
    /// `FillKind::SHADOW_DROP|SHADOW_INSET`. Shadow params
    /// (`offset.x, offset.y, σ, _unused`) ride in `fill_axis`.
    #[inline]
    pub(crate) fn draw_shadow(
        &mut self,
        rect: Rect,
        radius: Corners,
        color: ColorF16,
        fill_kind: FillKind,
        fill_axis: FillAxis,
    ) {
        let payload = DrawRectPayload {
            rect,
            radius,
            fill: color,
            stroke_color: ColorF16::TRANSPARENT,
            stroke_width: 0.0,
            fill_kind,
            fill_grad_idx: 0,
            fill_axis,
        };
        // Module-level noop policy: same payload predicate as
        // `draw_rect`. Catches shadow whose tint decayed to
        // transparent (or `Shadow::NONE`'s lerp endpoint) and
        // zero-extent paint rects.
        if payload.is_noop() {
            return;
        }
        self.record_start(CmdKind::DrawRect);
        write_pod(&mut self.data, payload);
    }

    #[inline]
    pub(crate) fn draw_text(&mut self, rect: Rect, color: ColorF16, key: TextCacheKey) {
        let payload = DrawTextPayload { rect, color, key };
        if payload.is_noop() {
            return;
        }
        self.record_start(CmdKind::DrawText);
        write_pod(&mut self.data, payload);
    }

    /// Record a `DrawMesh` cmd against already-staged vertices + indices
    /// in `shape_payloads.meshes`. Caller pushes verts (translated into
    /// the owner's logical-px world coords) and indices directly so the
    /// encoder can apply the owner-rect offset inline without an
    /// intermediate scratch buffer.
    pub(crate) fn draw_mesh(&mut self, payload: DrawMeshPayload) {
        if payload.is_noop() {
            return;
        }
        self.record_start(CmdKind::DrawMesh);
        write_pod(&mut self.data, payload);
    }

    /// Record a `DrawPolyline` cmd against already-staged points and
    /// colors. Caller pushes onto `polyline_points` / `polyline_colors`
    /// directly (so the encoder can apply the owner-rect offset
    /// inline without an intermediate scratch buffer) and passes the
    /// resulting spans here. The `color_mode`-dictated `colors_len`
    /// is a caller invariant enforced upstream by
    /// `PolylineColors::assert_matches` in `Shapes::add`.
    pub(crate) fn draw_polyline(&mut self, payload: DrawPolylinePayload) {
        if payload.is_noop() {
            return;
        }
        self.record_start(CmdKind::DrawPolyline);
        write_pod(&mut self.data, payload);
    }

    /// Lower a `BrushSource` into payload fields. Solid: pass colour
    /// through, zero gradient slots. Gradient: zero `fill_color`, push
    /// the LUT key into the per-frame arena, encode kind + spread +
    /// per-variant geometry into `fill_axis`. Takes the borrow-form
    /// enum so the hot solid path never spills the 88-byte user-side
    /// `Brush` to the stack.
    fn pack_brush(&mut self, brush: BrushSource<'_>) -> BrushPack {
        match brush {
            BrushSource::Solid(c) => BrushPack {
                fill_color: c,
                fill_kind: FillKind::SOLID,
                fill_grad_idx: 0,
                fill_axis: FillAxis::ZERO,
            },
            BrushSource::Linear(g) => BrushPack {
                fill_color: ColorF16::TRANSPARENT,
                fill_kind: FillKind::linear(g.spread),
                fill_grad_idx: self.push_gradient_lut_key(g.stops, g.interp),
                fill_axis: g.axis(),
            },
            BrushSource::Radial(g) => BrushPack {
                fill_color: ColorF16::TRANSPARENT,
                fill_kind: FillKind::radial(g.spread),
                fill_grad_idx: self.push_gradient_lut_key(g.stops, g.interp),
                fill_axis: g.axis(),
            },
            BrushSource::Conic(g) => BrushPack {
                fill_color: ColorF16::TRANSPARENT,
                fill_kind: FillKind::conic(g.spread),
                fill_grad_idx: self.push_gradient_lut_key(g.stops, g.interp),
                fill_axis: g.axis(),
            },
        }
    }

    #[inline]
    fn push_gradient_lut_key(&mut self, stops: ArrayVec<[Stop; MAX_STOPS]>, interp: Interp) -> u32 {
        let idx = self.gradient_lut_keys.len() as u32;
        self.gradient_lut_keys
            .push(GradientLutKey { stops, interp });
        idx
    }

    #[inline]
    fn record_start(&mut self, kind: CmdKind) {
        self.starts.push(self.data.len() as u32);
        self.kinds.push(kind);
    }

    /// Read the payload at `start` (in u32 words) as `T`. Caller picks
    /// `T` based on `kinds[i]` — the symmetric `write_pod` at push time
    /// guarantees the bytes are valid for the kind's expected payload.
    #[inline]
    pub(crate) fn read<T: bytemuck::Pod>(&self, start: u32) -> T {
        let start = start as usize;
        let n_words = std::mem::size_of::<T>() / 4;
        assert!(start + n_words <= self.data.len());
        let words = &self.data[start..start + n_words];
        // `pod_read_unaligned` so payloads with align >4 (e.g.
        // `DrawTextPayload` via `TextCacheKey: u64`) work even though
        // the arena is `Vec<u32>` (4-byte aligned).
        bytemuck::pod_read_unaligned(bytemuck::cast_slice(words))
    }
}

// --- raw POD r/w on the u32 arena ----------------------------------

/// Append a `T` to the arena as `size_of::<T>() / 4` u32 words. `Pod`
/// guarantees no padding bytes — the reinterpretation as `&[u32]` is
/// sound because `align_of::<T>() % 4 == 0` for every payload we use
/// (all field alignments are multiples of 4).
#[inline]
fn write_pod<T: bytemuck::Pod>(data: &mut Vec<u32>, v: T) {
    data.extend_from_slice(bytemuck::cast_slice(std::slice::from_ref(&v)));
}
