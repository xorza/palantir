use super::buffer::RenderBuffer;
use super::composer::{ComposeParams, Composer};
use super::encoder::{RenderCmd, encode};
use crate::tree::Tree;

/// Front-end CPU pipeline: tree → encoded commands → composed buffer. Owns
/// every persistent allocation (the recorded `RenderCmd` vec, the output
/// `RenderBuffer`, the [`Composer`] with its scratch). No GPU handles —
/// feed `build`'s return into any backend.
#[derive(Default)]
pub struct Pipeline {
    cmds: Vec<RenderCmd>,
    composer: Composer,
    buffer: RenderBuffer,
}

impl Pipeline {
    pub fn new() -> Self {
        Self::default()
    }

    /// Encode the tree into commands, compose them into the buffer, return
    /// the buffer ready to submit.
    pub fn build(
        &mut self,
        tree: &Tree,
        layout: &crate::layout::LayoutResult,
        params: &ComposeParams,
    ) -> &RenderBuffer {
        encode(tree, layout, &mut self.cmds);
        self.composer.compose(&self.cmds, params, &mut self.buffer);
        &self.buffer
    }
}
