use crate::display::Display;
use crate::primitives::fill_wire::FillKind;
use crate::primitives::fill_wire::LutRow;
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

/// Output of `compose`: physical-px instances grouped by scissor region plus
/// the wgpu callback sidecar for composited `GpuView`s.
///
/// Contains no compose-time scratch. Owns
/// its allocations across frames so steady-state composing is alloc-free for
/// the output; reuse a single `RenderBuffer` and call
/// `compose(.., &mut buffer)` each frame.
#[derive(Debug)]
pub(crate) struct RenderBuffer {
    pub(crate) quads: Vec<Quad>,
    pub(crate) texts: Vec<TextRun>,
    /// Scene-wide mesh rows, SoA-stored. The underlying vertex/index
    /// bytes live in the frame's
    /// [`FrameArena::meshes`](crate::forest::frame_arena::FrameArena::meshes);
    /// each row's `draw` field carries spans into that arena, and the
    /// `instance` field carries the Pod GPU state the backend uploads
    /// verbatim (read as a contiguous `&[MeshInstance]` via
    /// `meshes.instance()`).
    pub(crate) meshes: Soa<MeshDrawRow>,
    pub(crate) groups: Vec<DrawGroup>,
    /// One entry per *batch* of text runs that share a single text-backend
    /// `prepare`/`render` call. The composer coalesces text across
    /// adjacent groups when paint-order is preserved (no occluding
    /// quad/mesh, no rounded-clip change) — collapsing many small
    /// draw calls into one. Each batch's `texts` span is contiguous
    /// in `RenderBuffer.texts` by composer construction; batches anchor
    /// to groups via `TextBatch.last_group`.
    pub(crate) text_batches: Vec<TextBatch>,
    /// One entry per *batch* of mesh draws. Currently one [`GroupBatch`]
    /// per group that emitted meshes (mesh batches don't span scissor
    /// boundaries since meshes have no per-run bounds). Schedule and
    /// backend treat meshes structurally like text — drained via the
    /// same cursor-walking pattern as `text_batches`.
    pub(crate) mesh_batches: Vec<GroupBatch>,
    /// Scene-wide image rows, SoA-stored; structurally mirrors
    /// [`Self::meshes`]. The backend binds a per-handle texture and
    /// issues one draw per row (no shared vertex/index buffers — every
    /// quad is implicit four-corner from the shader's `vertex_index`).
    /// A `GpuView` is just another image row here — the scene carries
    /// no render-target concept; its off-screen target is listed
    /// separately in [`Self::frame_targets`], but the row composites
    /// exactly like an image: same `id` in the shared texture cache,
    /// same draw.
    pub(crate) images: Soa<ImageDrawRow>,
    /// `GpuView` off-screen targets to paint this frame — one per composited
    /// `GpuView` image row. The composer fills this directly from the
    /// `DrawImage.target` link (resolving the physical size + the app `paint`
    /// callback) as it walks image draws; the backend drains it to allocate +
    /// paint. Carries the callback, so the backend reaches the renderer without
    /// any `Ui`-side registry.
    pub(crate) frame_targets: Vec<RenderTargetDraw>,
    /// One entry per *batch* of image draws (currently one
    /// [`GroupBatch`] per group that emitted images). Schedule walks
    /// these in lockstep with `groups` via a cursor — same pattern as
    /// `text_batches` / `mesh_batches`.
    pub(crate) image_batches: Vec<GroupBatch>,
    /// Native GPU stroke instances + per-scissor-group batches. One
    /// [`GroupBatch`] per group that emitted strokes; the schedule walks
    /// them in lockstep with `mesh_batches` / `image_batches` via a
    /// cursor. Each instance is one [`CurveInstance`] basis kind —
    /// a `[t0, t1]` sub-range of a cubic/arc (adaptive count from
    /// on-screen length), a polyline segment, or joint chrome. The
    /// pipeline draws all instances in a batch with one non-indexed
    /// instanced draw.
    pub(crate) curves: Vec<CurveInstance>,
    pub(crate) curve_batches: Vec<GroupBatch>,
    /// Flat pool of rounded-clip mask geometry. `DrawGroup.rounded_clips`
    /// and `TextBatch.rounded_clips` are spans into it, each an
    /// outer→inner chain of the rounded masks active for that group /
    /// batch (nested rounded clips stack — the stencil path stamps one
    /// mask per chain entry). The composer pushes one chain per rounded
    /// `PushClip` (ancestors copied so every chain is contiguous);
    /// value-equal chains from separate pushes dedup at mask staging.
    pub(crate) rounded_clips: Vec<RoundedClip>,
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
    pub(crate) owner: RenderOwnerId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct RenderOwnerId(u64);

impl RenderOwnerId {
    pub(crate) fn reserve() -> Self {
        static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);
        Self(NEXT_OWNER.fetch_add(1, Ordering::Relaxed))
    }
}

impl RenderBuffer {
    pub(crate) fn new(owner: RenderOwnerId) -> Self {
        Self {
            owner,
            quads: Vec::new(),
            texts: Vec::new(),
            meshes: Soa::default(),
            groups: Vec::new(),
            text_batches: Vec::new(),
            mesh_batches: Vec::new(),
            images: Soa::default(),
            frame_targets: Vec::new(),
            image_batches: Vec::new(),
            curves: Vec::new(),
            curve_batches: Vec::new(),
            rounded_clips: Vec::new(),
            clear_override: None,
            viewport_phys: UVec2::ZERO,
            viewport_phys_f: Vec2::ZERO,
            scale: 1.0,
            time: Duration::ZERO,
        }
    }

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
        self.meshes.clear();
        self.images.clear();
        self.frame_targets.clear();
        self.groups.clear();
        self.text_batches.clear();
        self.mesh_batches.clear();
        self.image_batches.clear();
        self.curves.clear();
        self.curve_batches.clear();
        self.rounded_clips.clear();
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

/// A contiguous non-text draw range anchored to the group that drains it.
/// Mesh, image, and curve batches share this exact scheduling contract; the
/// owning `RenderBuffer` column determines what [`Self::items`] indexes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GroupBatch {
    pub(crate) items: Span,
    pub(crate) last_group: u32,
}

/// Above-text replay tiers in the backend's fixed intra-group order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PaintTier {
    Mesh,
    Image,
    Curve,
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

/// Chord-subdivisions per curve sub-instance. The shader expands one
/// instance into this many quads (= 2× this many triangles = 6× this
/// many indices). Has to stay in lockstep with the constant of the
/// same name in `curve.wgsl` (the curve pipeline stamps this value
/// into the shader source at module creation). Lives here, next to
/// [`CurveInstance`], because it's part of the composer↔backend wire
/// contract: the composer's sub-instance math and the backend's
/// per-instance vertex count both derive from it.
pub(crate) const SEGMENTS_PER_INSTANCE: u32 = 16;

/// Half-width of the antialiasing fringe every stroke adds beyond its
/// core half-width, physical px. Part of the CPU↔shader contract:
/// bbox inflation at shape lowering and the coverage math in
/// `curve.wgsl` (`HALF_FRINGE` there) both bake this value — bump
/// together.
pub(crate) const HALF_FRINGE: f32 = 0.5;

/// SVG-convention miter limit: a Miter join whose extension factor
/// `1/cos(half turn angle)` would exceed this renders as a bevel
/// instead (the composer downgrades the chrome kind). Pinned against
/// the const of the same name in `curve.wgsl`, which uses it to bound
/// the miter billboard extent.
pub(crate) const MITER_LIMIT: f32 = 4.0;

/// Basis tags for [`CurveInstance::kind`]. Pinned against the
/// `KIND_*` constants in `curve.wgsl` — bump together.
pub(crate) const CURVE_KIND_CUBIC: u32 = 0;
pub(crate) const CURVE_KIND_ARC: u32 = 1;
/// Straight polyline segment with bisector-clipped joint ends.
pub(crate) const CURVE_KIND_SEGMENT: u32 = 2;
/// Joint chrome billboards — the three `LineJoin` looks. Contiguous
/// values: the shader derives the fragment metric as
/// `kind - CURVE_KIND_JOIN_ROUND`.
pub(crate) const CURVE_KIND_JOIN_ROUND: u32 = 3;
pub(crate) const CURVE_KIND_JOIN_BEVEL: u32 = 4;
pub(crate) const CURVE_KIND_JOIN_MITER: u32 = 5;

/// Per-curve-sub-instance GPU state, uploaded to a
/// `step_mode: Instance` vertex buffer. For the strip kinds the
/// shader evaluates the stroke's parametric basis (picked by `kind`)
/// at parameter `t = mix(t0, t1, segment / SEGMENTS_PER_INSTANCE)`
/// for `segment ∈ [0, SEGMENTS_PER_INSTANCE]`, derives the tangent's
/// perpendicular, and offsets by ±(width/2 + AA fringe) to build the
/// stroked strip. All geometry lanes are pre-transformed to
/// physical-px; `width` is also physical px. Colors are linear-RGBA
/// straight-alpha (same convention as `MeshVertex.color`); the
/// fragment shader premultiplies at output.
///
/// Lane meaning by `kind`:
/// - [`CURVE_KIND_CUBIC`] — `p0..p3` are the cubic control points.
/// - [`CURVE_KIND_ARC`] — `p0` = center, `p1.x` = radius,
///   `p2 = (a0, a1)` start/end angle in radians (screen convention:
///   0 = +x, y-down ⇒ increasing = clockwise); `p1.y`/`p3` unused.
///   The angle at `t` is `mix(a0, a1, t)` — exact circle, no cubic
///   approximation error, and gradient `t` tracks the sweep linearly.
/// - [`CURVE_KIND_SEGMENT`] — `p0`/`p3` are the segment endpoints;
///   `p1`/`p2` carry the pre-oriented bisector clip-plane normals
///   for the start/end joint (zero = cap end, no clip; "keep" is
///   `dot(x - endpoint, n) <= 0`). Joint ends are butt-faced and
///   fragment-clipped at those planes — the composer hands adjacent
///   segments exact negations of the same sum, so strips partition
///   their concave overlap exactly (no double blend on translucent
///   strokes), and the convex wedge is filled by a join-chrome
///   instance.
/// - `CURVE_KIND_JOIN_*` — `p0` = joint point; `p1 = -d_a`,
///   `p2 = d_b` (unit segment directions into/out of the joint,
///   pre-oriented as the face-plane keep normals). Expands to one
///   billboard quad; the fragment fills the wedge between the two
///   segment end faces with an exact per-kind metric (round: radial;
///   bevel: radial ∧ bevel half-plane; miter: max of the two
///   centerline distances).
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
    /// Stroke colour at `t = 0`. Zeroed when `fill_kind != 0`; the
    /// shader samples the LUT row instead.
    pub(crate) color0: ColorU8,
    /// Stroke colour at `t = 1` — the shader lerps `color0 → color1`
    /// along `t` (straight-alpha, like `PolylineColors::PerPoint`).
    /// Equal to `color0` for single-colour strokes.
    pub(crate) color1: ColorU8,
    /// Cap kind per end, packed: bits 0..8 = start cap, 8..16 = end
    /// cap (0 = Butt, 1 = Square, 2 = Round). Only the leading
    /// sub-instance (`t0 ≈ 0`) and trailing sub-instance (`t1 ≈ 1`)
    /// actually extend their geometry; interior sub-instances see
    /// this lane and skip cap extension. Polyline segments carry the
    /// user cap on true ends and Butt on joint ends.
    pub(crate) cap: u32,
    /// Brush kind tag. Low byte 0 = solid, 1 = linear. Spread mode
    /// would ride in bits 8..16 like the quad pipeline, but a curve's
    /// `t` is already clamped to [0, 1] by construction, so spread is
    /// a no-op here. `#[repr(transparent)]` over `u32`, so the GPU
    /// sees the same bytes the `Uint32` vertex attribute expects.
    pub(crate) fill_kind: FillKind,
    /// Atlas row when `fill_kind` is a gradient, else ignored.
    pub(crate) fill_lut_row: LutRow,
    /// Basis tag — one of the `CURVE_KIND_*` constants. Selects how
    /// the vertex shader interprets the geometry lanes (see struct
    /// docs).
    pub(crate) kind: u32,
}

/// Pack per-end cap kinds into the [`CurveInstance::cap`] lane.
#[inline]
pub(crate) fn cap_lanes(start: u32, end: u32) -> u32 {
    start | (end << 8)
}

#[cfg(test)]
mod tests {
    use super::{RenderBuffer, RenderOwnerId};

    #[test]
    fn render_owner_is_explicit_and_unique() {
        let first = RenderOwnerId::reserve();
        let second = RenderOwnerId::reserve();
        assert_ne!(first, second);

        let buffer = RenderBuffer::new(first);
        assert_eq!(buffer.owner, first);
    }
}
