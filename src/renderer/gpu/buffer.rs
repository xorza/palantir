use super::quad::Quad;
use crate::layout::types::span::Span;
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
    /// `true` iff the encoder emitted at least one `PushClipRounded` this
    /// frame. Backends use this to lazy-init / select the stencil-mask
    /// render path; apps that never use rounded clip stay on the cheap
    /// scissor-only path.
    pub(crate) has_rounded_clip: bool,
}

impl Default for RenderBuffer {
    fn default() -> Self {
        Self {
            quads: Vec::new(),
            texts: Vec::new(),
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
    /// When set, the active clip is a rounded scissor — `scissor` is its
    /// bounding rect (in physical px) and `radius` is the per-corner
    /// radii (also physical px, already DPR-scaled). Backend writes a
    /// rounded SDF mask into stencil before drawing this group's
    /// quads/text and uses stencil-test pipelines for the draws. `None`
    /// = plain scissor (existing fast path).
    pub(crate) rounded_clip: Option<RoundedClipPhys>,
    pub(crate) quads: Span,
    pub(crate) texts: Span,
}

/// Physical-px rounded-clip descriptor riding on `DrawGroup`. Same rect
/// the scissor uses, plus per-corner radii in physical pixels (already
/// scaled by DPR). Backend feeds it into the mask-write quad pipeline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RoundedClipPhys {
    pub(crate) rect: URect,
    pub(crate) radius: Corners,
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
