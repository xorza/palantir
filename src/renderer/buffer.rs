use super::quad::Quad;
use crate::primitives::{Color, Rect, URect};
use crate::text::TextCacheKey;
use std::ops::Range;

/// Pure output of `compose`: physical-px instances grouped by scissor region,
/// ready for any rasterizing backend (wgpu, software, headless test capture).
///
/// Contains no GPU handles, no compose-time scratch — just the result. Owns
/// its allocations across frames so steady-state composing is alloc-free for
/// the output; reuse a single `RenderBuffer` and call
/// `compose(.., &mut buffer)` each frame.
pub struct RenderBuffer {
    pub quads: Vec<Quad>,
    pub texts: Vec<TextRun>,
    pub groups: Vec<DrawGroup>,
    /// Physical-px viewport, ceil'd. Backends use this as the default scissor
    /// when a group has no clip.
    pub viewport_phys: [u32; 2],
    /// Same viewport in float — needed by the wgpu vertex shader uniform.
    pub viewport_phys_f: [f32; 2],
    /// Logical→physical conversion factor, propagated from `ComposeParams`.
    /// Glyph rasterization needs it: shaped buffers are sized in logical px,
    /// so glyphon scales by this when emitting glyph quads.
    pub scale: f32,
}

impl Default for RenderBuffer {
    fn default() -> Self {
        Self {
            quads: Vec::new(),
            texts: Vec::new(),
            groups: Vec::new(),
            viewport_phys: [0, 0],
            viewport_phys_f: [0.0, 0.0],
            scale: 1.0,
        }
    }
}

impl RenderBuffer {
    pub fn new() -> Self {
        Self::default()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DrawGroup {
    pub scissor: Option<URect>,
    pub quads: Range<u32>,
    pub texts: Range<u32>,
}

/// One shaped text run placed in physical-px space. The buffer it references
/// is resolved by the backend at submit time using [`TextCacheKey`] against
/// the active `TextMeasure`.
#[derive(Clone, Copy, Debug)]
pub struct TextRun {
    /// Top-left of the run's bounding box, physical px.
    pub origin: [f32; 2],
    /// Bounds for clipping (physical px) — the parent rect after transform &
    /// snap. Glyphs outside are clipped by the backend even if the scissor
    /// rect is wider.
    pub bounds: URect,
    pub color: Color,
    pub key: TextCacheKey,
}

impl TextRun {
    pub fn rect(self) -> Rect {
        // Origin only — the size is implicit in the shaped buffer. Provided
        // for backends that want a logical bounding box.
        Rect {
            min: glam::Vec2::new(self.origin[0], self.origin[1]),
            size: crate::primitives::Size::new(self.bounds.w as f32, self.bounds.h as f32),
        }
    }
}
