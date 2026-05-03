//! Frontend (CPU) rendering pipeline.
//!
//! 1. [`encode`] — `&Tree` → `Vec<RenderCmd>` (logical-px). Pure free fn.
//! 2. [`Composer`] — `&[RenderCmd]` → `RenderBuffer` (physical-px quads
//!    + scissor groups). Owns the output + scratch; no GPU handles.
//! 3. [`Frontend`] (this struct) — orchestrates (1) + (2) and owns every
//!    persistent per-frame allocation. `Ui::end_frame` calls
//!    [`Frontend::build`] once and pulls the painted output via
//!    [`FrameOutput`].
//!
//! Output crosses into the backend as `&RenderBuffer` (defined one
//! level up so it sits at the frontend↔backend contract line).

mod cmd_buffer;
mod composer;
mod encoder;

pub use cmd_buffer::{RenderCmd, RenderCmdBuffer};
pub use composer::Composer;
pub use encoder::encode;

use crate::cascade::CascadeResult;
use crate::layout::LayoutResult;
use crate::primitives::{Display, Rect};
use crate::renderer::buffer::RenderBuffer;
use crate::tree::Tree;

/// One frame's CPU output: the composed render buffer and the damage
/// rect to scissor it to. Returned from [`Ui::frame`] after
/// [`Ui::end_frame`] has run, consumed by [`WgpuBackend::submit`].
///
/// `damage = None` means full repaint (first frame, post-resize, no
/// diff, or damage area exceeds the 50% threshold).
/// `damage = Some(rect)` means partial repaint scissored to that rect.
///
/// [`Ui::frame`]: crate::ui::Ui::frame
/// [`Ui::end_frame`]: crate::ui::Ui::end_frame
/// [`WgpuBackend::submit`]: crate::renderer::WgpuBackend::submit
pub struct FrameOutput<'a> {
    pub buffer: &'a RenderBuffer,
    pub damage: Option<Rect>,
}

/// CPU paint stage: tree → encoded commands → composed buffer. Owns
/// every persistent allocation (the recorded `RenderCmd` vec, the
/// output `RenderBuffer`, the [`Composer`] with its scratch) plus the
/// paint-time theme constants the encoder reads. No GPU handles —
/// `buffer()` is fed into any backend (`WgpuBackend`, future
/// software/Vello/etc.).
///
/// Lives inside [`Ui`](crate::ui::Ui) so a host gets the entire CPU
/// frame state (UI logic + paint output) from one
/// [`Ui::end_frame`](crate::ui::Ui::end_frame) call.
pub struct Frontend {
    cmds: RenderCmdBuffer,
    composer: Composer,
    buffer: RenderBuffer,
    /// `Theme::disabled_dim` cached here so `build` doesn't have to
    /// thread it through. `Ui::end_frame` writes this once per frame
    /// before calling `build`. Default matches `Theme::default`.
    disabled_dim: f32,
}

impl Default for Frontend {
    fn default() -> Self {
        Self {
            cmds: RenderCmdBuffer::default(),
            composer: Composer::default(),
            buffer: RenderBuffer::default(),
            disabled_dim: 0.5,
        }
    }
}

impl Frontend {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push the paint-time theme constants the encoder reads. Call
    /// once per frame before `build` (cheap; copies a few floats).
    pub fn set_disabled_dim(&mut self, dim: f32) {
        self.disabled_dim = dim;
    }

    /// Encode the tree into commands, compose them into the buffer.
    /// `damage_filter` is the filtered damage rect from
    /// `Damage::compute`.
    pub fn build(
        &mut self,
        tree: &Tree,
        layout: &LayoutResult,
        cascades: &CascadeResult,
        damage_filter: Option<Rect>,
        display: &Display,
    ) {
        encode(
            tree,
            layout,
            cascades,
            self.disabled_dim,
            damage_filter,
            &mut self.cmds,
        );
        self.composer.compose(&self.cmds, display, &mut self.buffer);
    }

    pub fn buffer(&self) -> &RenderBuffer {
        &self.buffer
    }
}
