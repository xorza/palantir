use super::quad::Quad;
use crate::layout::types::span::Span;
use crate::primitives::mesh::Mesh;
use crate::primitives::{color::Color, corners::Corners, rect::Rect, urect::URect};
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
    pub(crate) meshes: MeshScene,
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
}

impl RenderBuffer {
    /// `true` iff at least one group carries a rounded clip. Backends
    /// use this to decide whether to walk the stencil-mask path. Cheap
    /// linear scan over groups (typically a handful).
    pub(crate) fn has_rounded_clip(&self) -> bool {
        self.groups.iter().any(|g| g.rounded_clip.is_some())
    }
}

impl Default for RenderBuffer {
    fn default() -> Self {
        Self {
            quads: Vec::new(),
            texts: Vec::new(),
            meshes: MeshScene::default(),
            groups: Vec::new(),
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
    pub(crate) meshes: Span,
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
    pub(crate) arena: Mesh,
}

impl MeshScene {
    #[inline]
    pub(crate) fn clear(&mut self) {
        self.draws.clear();
        self.arena.clear();
    }
}

/// One mesh draw within a group. Vertex/index slices live in
/// `RenderBuffer.meshes.arena`. Tint was already baked into vertex
/// colors at compose time, so this entry carries just the spans.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MeshDraw {
    pub(crate) vertices: Span,
    pub(crate) indices: Span,
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
    /// Per-run scale factor on top of the global DPI scale, sourced from
    /// the cumulative ancestor `TranslateScale.scale` at compose time.
    /// `1.0` outside any transformed subtree. Multiplied into glyphon's
    /// per-`TextArea.scale` so a zoomed `Scroll` subtree paints
    /// proportionally larger glyphs without reshaping (linear upscale
    /// from the original glyph atlas — acceptable for transient zoom UI;
    /// a future quality bake-off could reshape at the new size).
    pub(crate) scale: f32,
}
