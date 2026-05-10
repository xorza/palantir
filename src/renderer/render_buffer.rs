use super::quad::Quad;
use crate::layout::types::span::Span;
use crate::primitives::mesh::MeshVertex;
use crate::primitives::{color::Color, corners::Corners, urect::URect};
use crate::text::TextCacheKey;
use glam::{UVec2, Vec2};

/// Pure output of `compose`: physical-px instances grouped by scissor region,
/// ready for any rasterizing backend (wgpu, software, headless test capture).
///
/// Contains no GPU handles, no compose-time scratch — just the result. Owns
/// its allocations across frames so steady-state composing is alloc-free for
/// the output; reuse a single `RenderBuffer` and call
/// `compose(.., &mut buffer)` each frame.
#[derive(Clone)]
pub(crate) struct RenderBuffer {
    pub(crate) quads: Vec<Quad>,
    pub(crate) texts: Vec<TextRun>,
    pub(crate) meshes: Vec<MeshDraw>,
    /// Physical-px vertex pool referenced by `meshes`. Indices in
    /// `mesh_indices` are vertex-local — the backend issues
    /// `draw_indexed` with the appropriate `base_vertex`.
    pub(crate) mesh_vertices: Vec<MeshVertex>,
    pub(crate) mesh_indices: Vec<u16>,
    pub(crate) groups: Vec<DrawGroup>,
    /// Physical-px viewport, ceil'd. Backends use this as the default scissor
    /// when a group has no clip.
    pub(crate) viewport_phys: UVec2,
    /// Same viewport in float — needed by the wgpu vertex shader uniform.
    pub(crate) viewport_phys_f: Vec2,
    /// Logical→physical conversion factor, propagated from `Display`.
    /// Glyph rasterization needs it: shaped buffers are sized in logical px,
    /// so glyphon scales by this when emitting glyph quads.
    pub(crate) scale: f32,
    /// `true` iff the composer emitted at least one rounded-clip group
    /// this frame. Set in the `PushClipRounded` branch; backend reads
    /// once to lazy-init / select the stencil-mask render path. Apps
    /// that never round-clip never touch the stencil path.
    pub(crate) has_rounded_clip: bool,
}

impl Default for RenderBuffer {
    fn default() -> Self {
        Self {
            quads: Vec::new(),
            texts: Vec::new(),
            meshes: Vec::new(),
            mesh_vertices: Vec::new(),
            mesh_indices: Vec::new(),
            groups: Vec::new(),
            viewport_phys: UVec2::ZERO,
            viewport_phys_f: Vec2::ZERO,
            scale: 1.0,
            has_rounded_clip: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawGroup {
    pub(crate) scissor: Option<URect>,
    /// When set, the active clip is a rounded scissor: `scissor` is the
    /// mask's bounding rect and `rounded_clip` carries the per-corner
    /// radii in physical px (DPR-scaled). Backend stamps the mask using
    /// `(scissor, rounded_clip)` and switches to stencil-test pipelines
    /// for this group's draws. `None` = plain scissor.
    pub(crate) rounded_clip: Option<Corners>,
    pub(crate) quads: Span,
    pub(crate) texts: Span,
    pub(crate) meshes: Span,
}

/// One mesh draw within a group. Vertex/index slices live in
/// `RenderBuffer.mesh_vertices` / `.mesh_indices`. `tint` is a
/// per-draw scalar multiplied into every vertex color in the shader.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MeshDraw {
    pub(crate) vertices: Span,
    pub(crate) indices: Span,
    pub(crate) tint: Color,
}

/// One shaped text run placed in physical-px space. The buffer it references
/// is resolved by the backend at submit time using [`TextCacheKey`] against
/// the active `TextMeasure`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TextRun {
    /// Top-left of the run's bounding box, physical px.
    pub(crate) origin: Vec2,
    /// Bounds for clipping (physical px) — the parent rect after transform &
    /// snap. Glyphs outside are clipped by the backend even if the scissor
    /// rect is wider.
    pub(crate) bounds: URect,
    pub(crate) color: Color,
    pub(crate) key: TextCacheKey,
}
