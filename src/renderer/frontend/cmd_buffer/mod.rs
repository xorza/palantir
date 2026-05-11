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

use crate::primitives::mesh::MeshVertex;
use crate::primitives::{
    color::Color, corners::Corners, rect::Rect, stroke::Stroke, transform::TranslateScale,
};
use crate::shape::ShapeArenas;
use crate::text::TextCacheKey;
use glam::Vec2;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CmdKind {
    PushClip,
    /// Scissor clip + rounded-corner stencil mask. Carries
    /// `PushClipRoundedPayload` (rect + radius). Composer treats it as
    /// a regular scissor for the purposes of group splitting; the
    /// backend's stencil path reads the radius to write the SDF mask.
    PushClipRounded,
    PopClip,
    PushTransform,
    PopTransform,
    DrawRect,
    DrawRectStroked,
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

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct PushClipRoundedPayload {
    pub(crate) rect: Rect,
    pub(crate) radius: Corners,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct DrawRectPayload {
    pub(crate) rect: Rect,
    pub(crate) radius: Corners,
    pub(crate) fill: Color,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct DrawRectStrokedPayload {
    pub(crate) rect: Rect,
    pub(crate) radius: Corners,
    pub(crate) fill: Color,
    pub(crate) stroke: Stroke,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct DrawTextPayload {
    pub(crate) rect: Rect,
    pub(crate) color: Color,
    pub(crate) key: TextCacheKey,
}

/// Mesh draw payload (40 B). Spans are inlined as `(start, len)`
/// `u32` pairs so the payload is plain Pod — no `Span: Pod` needed.
/// `origin` is the logical-px translation applied at compose time
/// before baking the transform/DPI into physical-px verts.
/// Stroked polyline payload (40 B). `width` is logical px; the
/// composer scales it through the active transform + DPI before
/// tessellation. Points + colors live in
/// [`RenderCmdBuffer::polyline_points`] /
/// [`RenderCmdBuffer::polyline_colors`]; `colors_len` is 1
/// (broadcast), `points_len` (per-point), or `points_len - 1`
/// (per-segment), selected by `color_mode` (a [`ColorMode`]
/// promoted to `u32` for Pod alignment).
///
/// `bbox` is the axis-aligned bounds of `points` in **logical
/// (cmd-buffer) coords** — no width inflation, no transform
/// applied. Computed by the encoder in a single pass while it
/// streams points into the arena, so adding it is a constant-time
/// cost regardless of point count. Composer transforms the 4
/// corners (uniform-scale `TranslateScale` preserves AABBs),
/// inflates by the physical-px outer-fringe offset, and
/// short-circuits the per-point transform when the result misses
/// the active scissor.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct DrawPolylinePayload {
    pub(crate) bbox: Rect,
    pub(crate) width: f32,
    pub(crate) color_mode: u32,
    pub(crate) points_start: u32,
    pub(crate) points_len: u32,
    pub(crate) colors_start: u32,
    pub(crate) colors_len: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct DrawMeshPayload {
    pub(crate) origin: Vec2,
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
    /// Self-contained per-variant geometry. `DrawMesh` /
    /// `DrawPolyline` payload spans slice into the arenas inside
    /// this. Self-containment is load-bearing: a future encode
    /// cache snapshots a sub-range of `kinds`/`starts`/`data` plus
    /// a copy of this struct, so replay doesn't need the original
    /// `Tree` arenas around. See [`ShapeArenas`].
    pub(crate) shape_arenas: ShapeArenas,
}

impl RenderCmdBuffer {
    pub(crate) fn clear(&mut self) {
        self.kinds.clear();
        self.starts.clear();
        self.data.clear();
        self.shape_arenas.clear();
    }

    #[inline]
    pub(crate) fn push_clip(&mut self, r: Rect) {
        self.record_start(CmdKind::PushClip);
        write_pod(&mut self.data, r);
    }

    #[inline]
    pub(crate) fn push_clip_rounded(&mut self, rect: Rect, radius: Corners) {
        self.record_start(CmdKind::PushClipRounded);
        write_pod(&mut self.data, PushClipRoundedPayload { rect, radius });
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
    pub(crate) fn draw_rect(&mut self, rect: Rect, radius: Corners, fill: Color, stroke: Stroke) {
        // Two cmd kinds keep the wire format compact: a stroke-less
        // rect skips the trailing 24 B of stroke payload. The branch
        // is on the value, not on `Option`-presence — semantically
        // identical, no Option machinery upstream.
        if stroke.is_noop() {
            self.record_start(CmdKind::DrawRect);
            write_pod(&mut self.data, DrawRectPayload { rect, radius, fill });
        } else {
            self.record_start(CmdKind::DrawRectStroked);
            write_pod(
                &mut self.data,
                DrawRectStrokedPayload {
                    rect,
                    radius,
                    fill,
                    stroke,
                },
            );
        }
    }

    #[inline]
    pub(crate) fn draw_text(&mut self, rect: Rect, color: Color, key: TextCacheKey) {
        self.record_start(CmdKind::DrawText);
        write_pod(&mut self.data, DrawTextPayload { rect, color, key });
    }

    /// Copy `verts` + `idx` into the cmd buffer's mesh arena and
    /// record a `DrawMesh` cmd. Indices are pushed unchanged; the
    /// composer (and ultimately the wgpu `draw_indexed`) addresses
    /// the vertex range with a `base_vertex` offset.
    pub(crate) fn draw_mesh(
        &mut self,
        origin: Vec2,
        tint: Color,
        verts: &[MeshVertex],
        idx: &[u16],
    ) {
        let mesh = &mut self.shape_arenas.meshes;
        let v_start = mesh.vertices.len() as u32;
        mesh.vertices.extend_from_slice(verts);
        let i_start = mesh.indices.len() as u32;
        mesh.indices.extend_from_slice(idx);
        self.record_start(CmdKind::DrawMesh);
        write_pod(
            &mut self.data,
            DrawMeshPayload {
                origin,
                tint,
                v_start,
                v_len: verts.len() as u32,
                i_start,
                i_len: idx.len() as u32,
            },
        );
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
