use super::quad::Quad;
use crate::layout::types::span::Span;
use crate::primitives::mesh::Mesh;
use crate::primitives::{color::ColorU8, corners::Corners, rect::Rect, urect::URect};
use crate::renderer::gradient_atlas::GradientCpuAtlas;
use crate::text::TextCacheKey;
use glam::{UVec2, Vec2};

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
    /// Cross-frame gradient LUT atlas. Composer registers each
    /// `Brush::Linear` it encounters during compose; backend drains
    /// the dirty marker via `flush()` and uploads when populated.
    /// Persistent: rows stay baked across frames so repeated authoring
    /// of the same gradient is O(1) hash lookup.
    pub(crate) gradient_atlas: GradientCpuAtlas,
}

impl Default for RenderBuffer {
    fn default() -> Self {
        Self {
            quads: Vec::new(),
            texts: Vec::new(),
            meshes: MeshScene::default(),
            groups: Vec::new(),
            text_batches: Vec::new(),
            has_rounded_clip: false,
            viewport_phys: UVec2::ZERO,
            viewport_phys_f: Vec2::ZERO,
            scale: 1.0,
            gradient_atlas: GradientCpuAtlas::default(),
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
    pub(crate) meshes: Span,
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

/// Scene-wide mesh pool: per-draw entries plus the shared vertex/index
/// arena they slice into. Bundled so the three columns — which are
/// always cleared, grown, and uploaded as a unit — can't drift.
#[derive(Default, Clone)]
pub(crate) struct MeshScene {
    pub(crate) draws: Vec<MeshDraw>,
    /// Parallels `draws`: one row per draw, uploaded verbatim to the
    /// per-instance vertex buffer. Composer pushes both together; the
    /// backend looks them up by `instance_index`.
    pub(crate) instances: Vec<MeshInstance>,
    pub(crate) arena: Mesh,
}

impl MeshScene {
    #[inline]
    pub(crate) fn clear(&mut self) {
        self.draws.clear();
        self.instances.clear();
        self.arena.clear();
    }
}

/// One mesh draw within a group. Vertex/index slices live in
/// `RenderBuffer.meshes.arena`; the per-instance transform + tint live
/// in [`MeshScene::instances`] at the matching index.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MeshDraw {
    pub(crate) vertices: Span,
    pub(crate) indices: Span,
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
