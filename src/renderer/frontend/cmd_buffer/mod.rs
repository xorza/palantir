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

use crate::forest::shapes::ShapePayloads;
use crate::primitives::brush::{Brush, FillAxis, Interp, MAX_STOPS, Stop};
use crate::primitives::{
    color::Color, corners::Corners, rect::Rect, stroke::Stroke, transform::TranslateScale,
};
use crate::renderer::quad::FillKind;
use crate::shape::{ColorModeBits, LineCapBits, LineJoinBits};
use crate::text::TextCacheKey;
use tinyvec::ArrayVec;

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
/// the brush's `axis()`. `fill: Color` is the solid colour when
/// `kind == SOLID`; for gradients it's zeroed and the composer's atlas
/// lookup supplies the LUT row.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct DrawRectPayload {
    pub(crate) rect: Rect,
    pub(crate) radius: Corners,
    pub(crate) fill: Color,
    pub(crate) stroke_color: Color,
    pub(crate) stroke_width: f32,
    pub(crate) fill_kind: FillKind,
    pub(crate) fill_grad_idx: u32,
    pub(crate) fill_axis: FillAxis,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct DrawTextPayload {
    pub(crate) rect: Rect,
    pub(crate) color: Color,
    pub(crate) key: TextCacheKey,
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

/// Mesh draw payload. Spans are inlined as `(start, len)` u32 pairs so
/// the payload is plain Pod — no `Span: Pod` needed. Vertex positions
/// are already in logical-px world-coords (encoder pre-translates by
/// the owner's top-left), matching the polyline convention.
#[padding_struct::padding_struct]
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct DrawMeshPayload {
    pub(crate) tint: Color,
    pub(crate) v_start: u32,
    pub(crate) v_len: u32,
    pub(crate) i_start: u32,
    pub(crate) i_len: u32,
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
    fill_color: Color,
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
    pub(crate) fn draw_rect(&mut self, rect: Rect, radius: Corners, fill: &Brush, stroke: Stroke) {
        // Stroke stays solid-only — gradient strokes are a non-goal.
        let BrushPack {
            fill_color,
            fill_kind,
            fill_grad_idx,
            fill_axis,
        } = self.pack_brush(fill);

        let (stroke_color, stroke_width) = if stroke.is_noop() {
            (Color::TRANSPARENT, 0.0)
        } else {
            (
                stroke.brush.as_solid().expect(
                    "gradient brush rendering not yet implemented; see docs/roadmap/brushes.md slice 2",
                ),
                stroke.width,
            )
        };
        self.record_start(CmdKind::DrawRect);
        write_pod(
            &mut self.data,
            DrawRectPayload {
                rect,
                radius,
                fill: fill_color,
                stroke_color,
                stroke_width,
                fill_kind,
                fill_grad_idx,
                fill_axis,
            },
        );
    }

    #[inline]
    pub(crate) fn draw_text(&mut self, rect: Rect, color: Color, key: TextCacheKey) {
        self.record_start(CmdKind::DrawText);
        write_pod(&mut self.data, DrawTextPayload { rect, color, key });
    }

    /// Record a `DrawMesh` cmd against already-staged vertices + indices
    /// in `shape_payloads.meshes`. Caller pushes verts (translated into
    /// the owner's logical-px world coords) and indices directly so the
    /// encoder can apply the owner-rect offset inline without an
    /// intermediate scratch buffer.
    pub(crate) fn draw_mesh(&mut self, payload: DrawMeshPayload) {
        self.record_start(CmdKind::DrawMesh);
        write_pod(&mut self.data, payload);
    }

    /// Record a `DrawPolyline` cmd against already-staged points and
    /// colors. Caller pushes onto `polyline_points` / `polyline_colors`
    /// directly (so the encoder can apply the owner-rect offset
    /// inline without an intermediate scratch buffer) and passes the
    /// resulting spans here. `points_len >= 2` and the
    /// `color_mode`-dictated `colors_len` are caller invariants —
    /// `Shape::is_noop` and `lower_polyline` enforce them upstream.
    pub(crate) fn draw_polyline(&mut self, payload: DrawPolylinePayload) {
        self.record_start(CmdKind::DrawPolyline);
        write_pod(&mut self.data, payload);
    }

    /// Lower a `Brush` into payload fields. Solid: pass colour through,
    /// zero gradient slots. Gradient: zero `fill_color`, push the LUT
    /// key into the per-frame arena, encode kind + spread + per-variant
    /// geometry into `fill_axis`.
    fn pack_brush(&mut self, brush: &Brush) -> BrushPack {
        match brush {
            Brush::Solid(c) => BrushPack {
                fill_color: *c,
                fill_kind: FillKind::SOLID,
                fill_grad_idx: 0,
                fill_axis: FillAxis::ZERO,
            },
            Brush::Linear(g) => BrushPack {
                fill_color: Color::TRANSPARENT,
                fill_kind: FillKind::linear(g.spread),
                fill_grad_idx: self.push_gradient_lut_key(g.stops, g.interp),
                fill_axis: g.axis(),
            },
            Brush::Radial(g) => BrushPack {
                fill_color: Color::TRANSPARENT,
                fill_kind: FillKind::radial(g.spread),
                fill_grad_idx: self.push_gradient_lut_key(g.stops, g.interp),
                fill_axis: g.axis(),
            },
            Brush::Conic(g) => BrushPack {
                fill_color: Color::TRANSPARENT,
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
