use crate::display::Display;
use crate::primitives::paint::FillKind;
use crate::primitives::paint::LutRow;
use crate::primitives::span::Span;
use crate::primitives::{color::Color, color::ColorU8, corners::Corners, rect::Rect, urect::URect};
use crate::renderer::gpu_view::GpuPaintRef;
use crate::renderer::quad::Quad;
use crate::renderer::texture_id::TextureId;
use crate::text::TextCacheKey;
use glam::{UVec2, Vec2};
use soa_rs::{Soa, Soars};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Pure output of `compose`: physical-px instances grouped by scissor region,
/// ready for any rasterizing backend (wgpu, software, headless test capture).
///
/// Contains no GPU handles, no compose-time scratch — just the result. Owns
/// its allocations across frames so steady-state composing is alloc-free for
/// the output; reuse a single `RenderBuffer` and call
/// `compose(.., &mut buffer)` each frame.
#[derive(Debug)]
pub(crate) struct RenderBuffer {
    pub(crate) quads: Vec<Quad>,
    pub(crate) texts: Vec<TextRun>,
    pub(crate) meshes: MeshScene,
    pub(crate) groups: Vec<DrawGroup>,
    /// One entry per *batch* of text runs that share a single text-backend
    /// `prepare`/`render` call. The composer coalesces text across
    /// adjacent groups when paint-order is preserved (no occluding
    /// quad/mesh, no rounded-clip change) — collapsing many small
    /// draw calls into one. Each batch's `texts` span is contiguous
    /// in `RenderBuffer.texts` by composer construction; batches anchor
    /// to groups via `TextBatch.last_group`.
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
    /// `GpuView` off-screen targets to paint this frame — one per composited
    /// `GpuView` image row. The composer fills this directly from the
    /// `DrawImage.target` link (resolving the physical size + the app `paint`
    /// callback) as it walks image draws; the backend drains it to allocate +
    /// paint. Carries the callback, so the backend reaches the renderer without
    /// any `Ui`-side registry.
    pub(crate) frame_targets: Vec<RenderTargetDraw>,
    /// One entry per *batch* of image draws (currently one
    /// `ImageBatch` per group that emitted images). Schedule walks
    /// these in lockstep with `groups` via a cursor — same pattern as
    /// `text_batches` / `mesh_batches`.
    pub(crate) image_batches: Vec<ImageBatch>,
    /// Native GPU curve instances + per-scissor-group batches. One
    /// `CurveBatch` per group that emitted curves; the schedule walks
    /// them in lockstep with `mesh_batches` / `image_batches` via a
    /// cursor. Each instance is a sub-range `[t0, t1]` of one cubic
    /// bezier — adaptive count: long / fast-curving inputs emit
    /// multiple instances at lowering time, smooth ones emit a single
    /// instance. The pipeline draws all instances in a batch with one
    /// non-indexed instanced draw.
    pub(crate) curves: Vec<CurveInstance>,
    pub(crate) curve_batches: Vec<CurveBatch>,
    /// Flat pool of rounded-clip mask geometry. `DrawGroup.rounded_clips`
    /// and `TextBatch.rounded_clips` are spans into it, each an
    /// outer→inner chain of the rounded masks active for that group /
    /// batch (nested rounded clips stack — the stencil path stamps one
    /// mask per chain entry). The composer pushes one chain per rounded
    /// `PushClip` (ancestors copied so every chain is contiguous);
    /// value-equal chains from separate pushes dedup at mask staging.
    pub(crate) rounded_clips: Vec<RoundedClip>,
    /// `true` iff at least one group carries a rounded clip — set by the
    /// composer when a `PushClip` carries a non-zero radius. Backend
    /// reads this to decide whether to walk the stencil-mask path;
    /// saves a linear scan over `groups` at submit time.
    pub(crate) has_rounded_clip: bool,
    /// Clear fold: when an unclipped opaque solid sharp quad covers the
    /// whole viewport, the composer discards everything composed before it
    /// (fully hidden), drops the quad, and records its fill here — the
    /// frame effectively starts at the last such cover. The backend clears
    /// (or pre-clears, on partial frames) to this color instead of the
    /// plan's — pixel-identical output, minus the hidden underlay and the
    /// full-surface fragment load of the biggest quad in the frame.
    pub(crate) clear_override: Option<Color>,
    /// Physical-px viewport, ceil'd. Backends use this as the default scissor
    /// when a group has no clip.
    pub(crate) viewport_phys: UVec2,
    /// Same viewport in float — needed by the wgpu vertex shader uniform.
    pub(crate) viewport_phys_f: Vec2,
    /// Logical→physical conversion factor, propagated from `Display`.
    /// Glyph rasterization needs it: shaped buffers are sized in logical px,
    /// so the text backend scales by this when emitting glyph quads.
    pub(crate) scale: f32,
    /// This frame's monotonic time (window-start `elapsed`), stamped by
    /// `Frontend::build` from `Ui::time` (not derivable from `Display`).
    /// The backend diffs it against each `GpuView`'s last paint to derive
    /// `GpuFrameCtx::dt`.
    pub(crate) time: Duration,
    /// Stable submitter identity, minted once at construction (one
    /// `RenderBuffer` per `Frontend`, i.e. per window) and never reset by
    /// `start_frame`. The shared backend's `ImagePipeline::paint_gpu_views`
    /// scopes `GpuView`-target eviction to it, so window A's submit can't
    /// free window B's targets.
    pub(crate) owner: u64,
}

impl Default for RenderBuffer {
    fn default() -> Self {
        // Global (not per-anything) so two windows' buffers can never
        // collide, whatever host constructed them.
        static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);
        Self {
            owner: NEXT_OWNER.fetch_add(1, Ordering::Relaxed),
            quads: Vec::new(),
            texts: Vec::new(),
            meshes: MeshScene::default(),
            groups: Vec::new(),
            text_batches: Vec::new(),
            mesh_batches: Vec::new(),
            images: ImageScene::default(),
            frame_targets: Vec::new(),
            image_batches: Vec::new(),
            curves: Vec::new(),
            curve_batches: Vec::new(),
            rounded_clips: Vec::new(),
            has_rounded_clip: false,
            clear_override: None,
            viewport_phys: UVec2::ZERO,
            viewport_phys_f: Vec2::ZERO,
            scale: 1.0,
            time: Duration::ZERO,
        }
    }
}

impl RenderBuffer {
    /// Reset every per-frame column (capacity retained) and stamp the
    /// frame's viewport + scale from `display`. Called by
    /// `Composer::compose` at frame start — the reset lives here,
    /// beside the fields, so adding a column forces choosing its reset
    /// in the same edit instead of in the composer's preamble.
    pub(crate) fn start_frame(&mut self, display: Display) {
        self.discard_scene();
        self.clear_override = None;
        self.viewport_phys = display.physical;
        self.viewport_phys_f = display.physical.as_vec2();
        self.scale = display.scale_factor;
        // Not derivable from `display`; `Frontend::build` stamps the real
        // value after compose.
        self.time = Duration::ZERO;
    }

    /// Drop every scene column (capacity retained), leaving the per-frame
    /// stamps (`clear_override`, viewport, scale, time) untouched. Shared by
    /// [`Self::start_frame`] and the composer's clear fold, which discards
    /// everything composed so far when a fullscreen opaque cover proves it
    /// invisible — a new scene column added here resets on both paths at once.
    pub(crate) fn discard_scene(&mut self) {
        self.quads.clear();
        self.texts.clear();
        self.meshes.rows.clear();
        self.images.rows.clear();
        self.frame_targets.clear();
        self.groups.clear();
        self.text_batches.clear();
        self.mesh_batches.clear();
        self.image_batches.clear();
        self.curves.clear();
        self.curve_batches.clear();
        self.rounded_clips.clear();
        self.has_rounded_clip = false;
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawGroup {
    pub(crate) scissor: Option<URect>,
    /// Outer→inner chain of rounded masks active for this group — a
    /// span into [`RenderBuffer::rounded_clips`]; empty = plain scissor.
    /// `scissor` is the rasterizer scissor (already clamped to viewport
    /// / ancestor scissors), while each chain entry carries the
    /// **unclamped** physical-px mask rect + per-corner radii used by
    /// the stencil-mask SDF. Keeping the mask rects unclamped is what
    /// prevents rounded corners from "sliding inward" into the visible
    /// region when the clipped node partially leaves the viewport — the
    /// SDF must always know the rect's true geometry; the scissor
    /// handles off-screen pixel rejection.
    pub(crate) rounded_clips: Span,
    pub(crate) quads: Span,
}

/// A coalesced batch of text runs sharing one text-backend `prepare` /
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
    /// Physical-pixel union of every contributing `TextRun.bounds`.
    /// The schedule sets this as the GPU scissor before the batch's
    /// `Text` step (intersected with `damage_scissor`) so glyphs can't
    /// rasterize outside any contributing run's bounds — long lines
    /// whose painted block grew past the per-group scissor (via
    /// ladder-snap or a wide owner rect) get clipped here. The
    /// shader does no per-fragment bounds test, so the GPU scissor
    /// is the only x-axis clip.
    pub(crate) scissor: URect,
    /// The rounded-mask chain every run in this batch was recorded
    /// under — a span into [`RenderBuffer::rounded_clips`], value-equal
    /// to `groups[last_group].rounded_clips` (a chain change closes the
    /// batch, so a batch never mixes masks). The schedule needs it when
    /// a batch drains past damage-skipped groups: the text must
    /// stencil-test against *this* chain, not whatever mask happens to
    /// be stamped at the drain point.
    pub(crate) rounded_clips: Span,
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
    pub(crate) corners: Corners,
}

/// Scene-wide mesh pool, SoA-stored as `Soa<MeshDrawRow>`. The
/// underlying vertex/index bytes live in the frame's
/// [`FrameArena::meshes`](crate::forest::frame_arena::FrameArena::meshes);
/// each row's `draw` field carries spans into that arena, and the
/// `instance` field carries the Pod GPU state the backend uploads
/// verbatim to the instance buffer (read as a contiguous
/// `&[MeshInstance]` via `rows.instance()` — same memory layout as
/// the previous parallel-`Vec` form).
#[derive(Debug, Default)]
pub(crate) struct MeshScene {
    pub(crate) rows: Soa<MeshDrawRow>,
}

/// Scene-wide image pool, SoA-stored as `Soa<ImageDrawRow>`. The
/// backend binds a per-handle texture and issues one draw per row
/// (no shared vertex/index buffers — every quad is implicit
/// four-corner from the shader's `vertex_index`).
///
/// A `GpuView` is just another image row here — the scene carries no
/// render-target concept. A `GpuView`'s off-screen target is listed
/// separately in [`RenderBuffer::frame_targets`] (the composer reads the
/// `DrawImage.target` link), but the row itself composites exactly like an
/// image: same `id` in the shared texture cache, same draw.
#[derive(Debug, Default)]
pub(crate) struct ImageScene {
    pub(crate) rows: Soa<ImageDrawRow>,
}

/// One `GpuView` off-screen target to paint this frame (see
/// [`RenderBuffer::frame_targets`]): the view's stable texture `id`, its used
/// physical size (`used` — the composed paint-rect size, ceiled ≥1, clamped
/// to the device max), and the app `paint` callback (threaded from
/// `Ui::gpu_views` through the cmd-buffer side-list, so the backend reaches the
/// renderer without a `Ui`-side registry). The backend allocates the target to
/// exactly `used` and runs `paint` into it before the main pass samples it.
#[derive(Clone, Debug)]
pub(crate) struct RenderTargetDraw {
    pub(crate) id: TextureId,
    pub(crate) used: UVec2,
    pub(crate) paint: GpuPaintRef,
}

/// One image draw row. Composer pushes one of these per image; the
/// SoA storage splits `id` and `instance` into their own contiguous
/// slices, so the backend uploads `rows.instance()` as a single
/// `write_buffer` and walks `rows.id()` for per-draw texture bindings.
/// `id` is the registration id behind an `ImageHandle`; the backend
/// looks it up in its GPU texture cache (and skips the draw on a miss).
#[derive(Soars, Clone, Copy, Debug, PartialEq)]
#[soa_derive(Debug)]
pub(crate) struct ImageDrawRow {
    pub id: TextureId,
    pub instance: ImageInstance,
}

/// Bit in [`ImageInstance::flags`]: wrap UVs with `fract` in the shader
/// (`ImageFit::Tile`).
pub(crate) const IMG_FLAG_TILED: u32 = 1 << 0;
/// Bit in [`ImageInstance::flags`]: nearest-neighbour sampling
/// (`ImageFilter::Nearest`) — the shader snaps the UV to the texel
/// center before the (linear-sampler) fetch, which lands the bilinear
/// weights exactly on one texel.
pub(crate) const IMG_FLAG_NEAREST: u32 = 1 << 1;

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
    /// UV crop extent (typically `(1, 1)`; smaller for `Cover` crop,
    /// `> 1` for `Tile` repeats). A `GpuView` ships `(1, 1)` — its target is
    /// sized exactly to the paint rect, so it samples the whole texture.
    pub(crate) uv_size: glam::Vec2,
    /// Linear-RGBA tint, premultiplied in the shader.
    pub(crate) tint: ColorU8,
    /// `IMG_FLAG_*` bits (tile wrap, nearest sampling). `u32` for a
    /// clean `Uint32` vertex attr.
    pub(crate) flags: u32,
}

/// One mesh draw within a group. Vertex/index slices live in the
/// frame's [`FrameArena::meshes`](crate::forest::frame_arena::FrameArena::meshes);
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
/// requirement is satisfied without filler. Color stores **straight-alpha
/// linear** bytes: the native text backend consumes linear and premultiplies
/// at output (no sRGB roundtrip — matches the crate's colour contract), which
/// keeps the per-frame hot path Pod-shaped and lets the backend hash whole
/// `TextRun` slices via `bytemuck`.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
// `pub` (not `pub(crate)`) is load-bearing: the text backend's gated
// `test_support` re-exports this via `pub use` so external benches can
// name it; a `pub(crate)` item can't be `pub use`d out of the crate.
pub struct TextRun {
    pub(crate) key: TextCacheKey,
    /// Top-left of the run's bounding box, physical px.
    pub(crate) origin: Vec2,
    /// Bounds for clipping (physical px) — the parent rect after transform &
    /// snap. The backend only y-culls whole lines against this (keeps
    /// off-screen lines out of the glyph atlas); the actual pixel clip is
    /// the batch GPU scissor ([`TextBatch::scissor`], the union of the
    /// batch's bounds), which the composer's strict-bounds batching rule
    /// keeps no wider than any ancestor-clipped run's bounds.
    pub(crate) bounds: URect,
    pub(crate) color: ColorU8,
    /// Per-run scale factor on top of the global DPI scale, sourced from
    /// the cumulative ancestor `TranslateScale.scale` at compose time
    /// and snapped to a log-multiplicative ladder
    /// (`composer::snap_text_scale`). `1.0` outside any transformed
    /// subtree. Multiplied into the text backend's per-`TextArea.scale`, which
    /// cosmic-text mixes into its glyph `CacheKey` (`font_size * scale`),
    /// so every distinct value here mints a fresh swash rasterization +
    /// atlas slot. Snapping is what keeps a continuous zoom gesture from
    /// re-rasterizing every glyph every frame.
    pub(crate) scale: f32,
}

impl std::hash::Hash for TextRun {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write(bytemuck::bytes_of(self));
    }
}

/// A batch of curve sub-instances emitted together. `instances` is a
/// contiguous range into [`CurveScene::instances`]; `last_group` is the
/// group whose iteration drains this batch in the schedule — mirrors
/// `MeshBatch.last_group` / `ImageBatch.last_group`. One batch per
/// scissor group with curves (no cross-group spanning — curves must
/// clip to the active scissor same as meshes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CurveBatch {
    pub(crate) instances: Span,
    pub(crate) last_group: u32,
}

/// Chord-subdivisions per curve sub-instance. The shader expands one
/// instance into this many quads (= 2× this many triangles = 6× this
/// many indices). Has to stay in lockstep with the constant of the
/// same name in `curve.wgsl` (the curve pipeline stamps this value
/// into the shader source at module creation). Lives here, next to
/// [`CurveInstance`], because it's part of the composer↔backend wire
/// contract: the composer's sub-instance math and the backend's
/// per-instance vertex count both derive from it.
pub(crate) const SEGMENTS_PER_INSTANCE: u32 = 16;

/// Per-curve-sub-instance GPU state, uploaded to a
/// `step_mode: Instance` vertex buffer. The shader evaluates the cubic
/// at parameter `t = mix(t0, t1, segment / SEGMENTS_PER_INSTANCE)` for
/// `segment ∈ [0, SEGMENTS_PER_INSTANCE]`, derives the tangent's
/// perpendicular, and offsets by ±(width/2 + AA fringe) to build the
/// stroked strip. All control points are pre-transformed to
/// physical-px; `width` is also physical px. Color is linear-RGBA
/// straight-alpha (same convention as `MeshVertex.color`); the
/// fragment shader premultiplies at output.
#[padding_struct::padding_struct]
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct CurveInstance {
    pub(crate) p0: Vec2,
    pub(crate) p1: Vec2,
    pub(crate) p2: Vec2,
    pub(crate) p3: Vec2,
    /// `[t0, t1]` — the sub-range of the parent curve this instance
    /// covers. The vertex shader subdivides this range into
    /// `SEGMENTS_PER_INSTANCE` chords; one curve emits ⌈N/16⌉
    /// sub-instances where `N` is the adaptive segment count.
    pub(crate) t0: f32,
    pub(crate) t1: f32,
    pub(crate) width: f32,
    /// Solid stroke colour. Zeroed when `fill_kind != 0`; the shader
    /// samples the LUT row instead.
    pub(crate) color: ColorU8,
    /// Cap kind tag — 0 = Butt, 1 = Square, 2 = Round. Only the
    /// leading sub-instance (`t0 ≈ 0`) and trailing sub-instance
    /// (`t1 ≈ 1`) actually extend their geometry; interior
    /// sub-instances see this lane and skip cap extension.
    pub(crate) cap: u32,
    /// Brush kind tag. Low byte 0 = solid, 1 = linear. Spread mode
    /// would ride in bits 8..16 like the quad pipeline, but a curve's
    /// `t` is already clamped to [0, 1] by construction, so spread is
    /// a no-op here. `#[repr(transparent)]` over `u32`, so the GPU
    /// sees the same bytes the `Uint32` vertex attribute expects.
    pub(crate) fill_kind: FillKind,
    /// Atlas row when `fill_kind` is a gradient, else ignored.
    pub(crate) fill_lut_row: LutRow,
}
