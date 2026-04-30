use super::quad::Quad;

/// Pure output of `compose`: physical-px instances grouped by scissor region,
/// ready for any rasterizing backend (wgpu, software, headless test capture).
///
/// Contains no GPU handles, no compose-time scratch — just the result. Owns
/// its allocations across frames so steady-state composing is alloc-free for
/// the output; reuse a single `RenderBuffer` and call
/// `compose(.., &mut buffer)` each frame.
#[derive(Default)]
pub struct RenderBuffer {
    pub quads: Vec<Quad>,
    pub groups: Vec<DrawGroup>,
    /// Physical-px viewport, ceil'd. Backends use this as the default scissor
    /// when a group has no clip.
    pub viewport_phys: [u32; 2],
    /// Same viewport in float — needed by the wgpu vertex shader uniform.
    pub viewport_phys_f: [f32; 2],
}

impl RenderBuffer {
    pub fn new() -> Self {
        Self::default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrawGroup {
    pub scissor: Option<ScissorRect>,
    pub start: u32,
    pub end: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScissorRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}
