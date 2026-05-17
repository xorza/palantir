use super::quad::Quad;
use crate::primitives::image::ImageHandle;
use crate::primitives::span::Span;
use crate::primitives::{color::ColorU8, corners::Corners, rect::Rect, urect::URect};
use crate::text::TextCacheKey;
use glam::{UVec2, Vec2};
use soa_rs::{Soa, Soars};

/// Pure output of `compose`: physical-px instances grouped by scissor region,
/// ready for any rasterizing backend (wgpu, software, headless test capture).
///
/// Contains no GPU handles, no compose-time scratch — just the result. Owns
/// its allocations across frames so steady-state composing is alloc-free for
/// the output; reuse a single `RenderBuffer` and call
/// `compose(.., &mut buffer)` each frame.
pub(crate) struct RenderBuffer {
    pub(crate) quads: Vec<Quad>,
    pub(crate) texts: Vec<TextRun>,
    pub(crate) meshes: MeshScene,
    pub(crate) groups: Vec<DrawGroup>,
    /// One entry per *batch* of text runs that share a single glyphon
    /// `prepare`/`render` call. The composer coalesces text across
    /// adjacent groups when paint-order is preserved (no occluding
    /// quad/mesh, no rounded-clip change) — collapsing many small
    /// draw calls into one. Each batch's `texts` span is contiguous
    /// in `RenderBuffer.texts` by composer construction. `DrawGroup`
    /// carries a `text_batch` index pointing here.
    pub(crate) text_batches: Vec<TextBatch>,
    /// One entry per *batch* of mesh draws. Currently one `MeshBatch`
    /// per group that emitted meshes (mesh batches don't span scissor
    /// boundaries since meshes have no per-run bounds). Schedule and
    /// backend treat meshes structurally like text — drained via the
    /// same cursor-walking pattern as `text_batches`.
    pub(crate) mesh_batches: Vec<MeshBatch>,
    /// Image draws + per-instance state. Structurally mirrors
    /// [`MeshScene`]; per-frame cleared in `compose`.
    pub(crate) images: ImageScene,
    /// One entry per *batch* of image draws (currently one
    /// `ImageBatch` per group that emitted images). Schedule walks
    /// these in lockstep with `groups` via a cursor — same pattern as
    /// `text_batches` / `mesh_batches`.
    pub(crate) image_batches: Vec<ImageBatch>,
    /// `true` iff at least one group carries a rounded clip — set by the
    /// composer when a `PushClip` carries a non-zero radius. Backend
    /// reads this to decide whether to walk the stencil-mask path;
    /// saves a linear scan over `groups` at submit time.
    pub(crate) has_rounded_clip: bool,
    /// Physical-px viewport, ceil'd. Backends use this as the default scissor
    /// when a group has no clip.
    pub(crate) viewport_phys: UVec2,
    /// Same viewport in float — needed by the wgpu vertex shader uniform.
    pub(crate) viewport_phys_f: Vec2,
    /// Logical→physical conversion factor, propagated from `Display`.
    /// Glyph rasterization needs it: shaped buffers are sized in logical px,
    /// so glyphon scales by this when emitting glyph quads.
    pub(crate) scale: f32,
}

impl Default for RenderBuffer {
    fn default() -> Self {
        Self {
            quads: Vec::new(),
            texts: Vec::new(),
            meshes: MeshScene::default(),
            groups: Vec::new(),
            text_batches: Vec::new(),
            mesh_batches: Vec::new(),
            images: ImageScene::default(),
            image_batches: Vec::new(),
            has_rounded_clip: false,
            viewport_phys: UVec2::ZERO,
            viewport_phys_f: Vec2::ZERO,
            scale: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawGroup {
    pub(crate) scissor: Option<URect>,
    /// When set, the active clip is a rounded scissor. `scissor` is the
    /// rasterizer scissor (already clamped to viewport / ancestor
    /// scissors), while `rounded_clip` carries the **unclamped**
    /// physical-px mask rect + per-corner radii used by the stencil-
    /// mask SDF. Keeping the mask rect unclamped is what prevents
    /// rounded corners from "sliding inward" into the visible region
    /// when the clipped node partially leaves the viewport — the SDF
    /// must always know the rect's true geometry; the scissor handles
    /// off-screen pixel rejection. `None` = plain scissor.
    pub(crate) rounded_clip: Option<RoundedClip>,
    pub(crate) quads: Span,
    pub(crate) texts: Span,
}

/// A coalesced batch of text runs sharing one `glyphon::prepare` /
/// `render` call. `texts` is a contiguous range into
/// `RenderBuffer.texts`. The schedule emits the render step at the
/// end of the batch's last group (after that group's quads), so any
/// quad in any group of the batch can underpaint the merged text.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TextBatch {
    pub(crate) texts: Span,
    /// Index into `RenderBuffer.groups` of the last group whose text
    /// contributed to this batch. The schedule emits the batch's
    /// `Text` step immediately after this group's quads draw, so any
    /// quad in any group of the batch underpaints the merged text.
    /// Intermediate groups with no text (e.g. a quad-only group
    /// between two text groups sharing one batch) can fall between
    /// the batch's `first_group` and `last_group`.
    pub(crate) last_group: u32,
}

/// A batch of mesh draws emitted together. `meshes` is a contiguous
/// range into `MeshScene.draws` (and parallel `instances`); `last_group`
/// is the group whose iteration drains this batch in the schedule —
/// mirrors `TextBatch.last_group`. Today's structural Phase 2 produces
/// one batch per group with meshes, so schedule iterates them via a
/// cursor in lockstep with the group loop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MeshBatch {
    pub(crate) meshes: Span,
    pub(crate) last_group: u32,
}

/// A batch of image draws emitted together. `images` is a contiguous
/// range into `ImageScene.draws` (and parallel `instances`);
/// `last_group` is the group whose iteration drains this batch in the
/// schedule — mirrors `MeshBatch`. Phase 5 emits one batch per group
/// with images; later slices can coalesce by texture handle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ImageBatch {
    pub(crate) images: Span,
    pub(crate) last_group: u32,
}

/// Physical-px rounded-clip geometry for stencil masking. `mask_rect`
/// is the clip's full physical-pixel rect — **not** clamped to viewport
/// or any ancestor scissor — so the mask SDF's corner curves stay
/// anchored at the rect's true edges even when the clip is partially
/// off-screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RoundedClip {
    pub(crate) mask_rect: Rect,
    pub(crate) radius: Corners,
}

/// Scene-wide mesh pool, SoA-stored as `Soa<MeshDrawRow>`. The
/// underlying vertex/index bytes live in the frame's
/// [`FrameArena::meshes`](crate::common::frame_arena::FrameArena::meshes);
/// each row's `draw` field carries spans into that arena, and the
/// `instance` field carries the Pod GPU state the backend uploads
/// verbatim to the instance buffer (read as a contiguous
/// `&[MeshInstance]` via `rows.instance()` — same memory layout as
/// the previous parallel-`Vec` form).
#[derive(Default)]
pub(crate) struct MeshScene {
    pub(crate) rows: Soa<MeshDrawRow>,
}

impl MeshScene {
    #[inline]
    pub(crate) fn clear(&mut self) {
        self.rows.clear();
    }
}

/// Scene-wide image pool, SoA-stored as `Soa<ImageDrawRow>`. The
/// backend binds a per-handle texture and issues one draw per row
/// (no shared vertex/index buffers — every quad is implicit
/// four-corner from the shader's `vertex_index`).
#[derive(Default)]
pub(crate) struct ImageScene {
    pub(crate) rows: Soa<ImageDrawRow>,
}

impl ImageScene {
    #[inline]
    pub(crate) fn clear(&mut self) {
        self.rows.clear();
    }
}

/// One image draw row. Composer pushes one of these per image; the
/// SoA storage splits `handle` and `instance` into their own
/// contiguous slices, so the backend uploads `rows.instance()` as a
/// single `write_buffer` and walks `rows.handle()` for per-draw
/// texture bindings.
#[derive(Soars, Clone, Copy, Debug, PartialEq)]
#[soa_derive(Debug)]
pub(crate) struct ImageDrawRow {
    pub handle: ImageHandle,
    pub instance: ImageInstance,
}

/// Per-image GPU state, uploaded to a `step_mode: Instance` vertex
/// buffer. Shader interpolates `uv_min + corner * uv_size` per fragment
/// (where `corner` is the four-corner `vertex_index`), samples the
/// texture, and multiplies by `tint`. `uv_min`+`uv_size` carry the
/// crop for `ImageFit::Cover`; the other fit modes ship `(0,0)+(1,1)`
/// and let the encoder shape the paint rect instead. `Pod`-shaped so
/// the upload is a single `write_buffer`.
#[padding_struct::padding_struct]
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ImageInstance {
    /// Physical-px paint rect.
    pub(crate) rect: Rect,
    /// UV crop top-left (0..1 texture coords).
    pub(crate) uv_min: glam::Vec2,
    /// UV crop extent (typically `(1, 1)`; smaller for `Cover` crop).
    pub(crate) uv_size: glam::Vec2,
    /// Linear-RGBA tint, premultiplied in the shader.
    pub(crate) tint: ColorU8,
}

/// One mesh draw within a group. Vertex/index slices live in the
/// frame's [`FrameArena::meshes`](crate::common::frame_arena::FrameArena::meshes);
/// the per-instance transform + tint live alongside as
/// [`MeshDrawRow::instance`] (same row in the SoA, separate column).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MeshDraw {
    pub(crate) vertices: Span,
    pub(crate) indices: Span,
}

/// One mesh draw row. SoA split keeps span info (`draw`) and Pod
/// instance state (`instance`) in their own contiguous columns so
/// the backend can upload `rows.instance()` as a single
/// `write_buffer` while still walking `rows.draw()` for per-draw
/// vertex/index span issue.
#[derive(Soars, Clone, Copy, Debug, PartialEq)]
#[soa_derive(Debug)]
pub(crate) struct MeshDrawRow {
    pub draw: MeshDraw,
    pub instance: MeshInstance,
}

/// Per-mesh GPU state, uploaded to a `step_mode: Instance` vertex
/// buffer. The shader composes `physical = pos * scale + translate`
/// and `out_color = vertex.color * tint`. `Pod`-shaped so the upload
/// is a single `write_buffer` of `bytemuck::cast_slice(instances)`.
#[padding_struct::padding_struct]
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct MeshInstance {
    pub(crate) translate: Vec2,
    pub(crate) scale: f32,
    pub(crate) tint: ColorU8,
}

/// One shaped text run placed in physical-px space. The buffer it references
/// is resolved by the backend at submit time using [`TextCacheKey`] against
/// the active `TextMeasure`.
///
/// **Layout**: fields ordered so the struct is `Pod` with no internal
/// padding. `TextCacheKey` (24 B, align 8) leads so its alignment
/// requirement is satisfied without filler. Color stores already-encoded
/// sRGB bytes (glyphon's `ColorMode::Accurate` consumes sRGB; doing the
/// conversion at compose time keeps the per-frame hot path Pod-shaped
/// and lets the backend hash whole `TextRun` slices via `bytemuck`).
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct TextRun {
    pub(crate) key: TextCacheKey,
    /// Top-left of the run's bounding box, physical px.
    pub(crate) origin: Vec2,
    /// Bounds for clipping (physical px) — the parent rect after transform &
    /// snap. Glyphs outside are clipped by the backend even if the scissor
    /// rect is wider.
    pub(crate) bounds: URect,
    pub(crate) color: ColorU8,
    /// Per-run scale factor on top of the global DPI scale, sourced from
    /// the cumulative ancestor `TranslateScale.scale` at compose time.
    /// `1.0` outside any transformed subtree. Multiplied into glyphon's
    /// per-`TextArea.scale` so a zoomed `Scroll` subtree paints
    /// proportionally larger glyphs without reshaping (linear upscale
    /// from the original glyph atlas — acceptable for transient zoom UI;
    /// a future quality bake-off could reshape at the new size).
    pub(crate) scale: f32,
}

impl std::hash::Hash for TextRun {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write(bytemuck::bytes_of(self));
    }
}
