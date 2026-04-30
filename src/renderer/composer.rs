use super::buffer::RenderBuffer;
use super::compose::{ComposeParams, compose};
use super::encoder::{RenderCmd, encode};
use crate::tree::Tree;

/// CPU-only owner of the encode + compose caches. Holds the `RenderCmd` vec
/// and the `RenderBuffer` so steady-state framing reuses every allocation.
/// No GPU handles — feed `&buffer()` to any backend.
#[derive(Default)]
pub struct Composer {
    cmds: Vec<RenderCmd>,
    buffer: RenderBuffer,
}

impl Composer {
    pub fn new() -> Self {
        Self::default()
    }

    /// One-shot: encode the tree into commands, compose into the buffer, and
    /// return the buffer ready to submit. Both stages reuse this composer's
    /// caches across frames.
    pub fn build(&mut self, tree: &Tree, params: &ComposeParams) -> &RenderBuffer {
        encode(tree, &mut self.cmds);
        compose(&self.cmds, params, &mut self.buffer);
        &self.buffer
    }

    pub fn buffer(&self) -> &RenderBuffer {
        &self.buffer
    }
}
