//! Frontend (CPU) rendering pipeline.
//!
//! 1. [`encode`] — `&Tree` → [`RenderCmdBuffer`](cmd_buffer::RenderCmdBuffer)
//!    (logical-px). Pure free fn.
//! 2. [`Composer`] — `&RenderCmdBuffer` → `RenderBuffer` (physical-px
//!    quads + scissor groups). Owns the output + scratch; no GPU handles.
//! 3. [`Frontend`] (this struct) — orchestrates (1) + (2) and owns every
//!    persistent per-frame allocation. [`Host`] calls [`Frontend::build`]
//!    once per frame and feeds the composed buffer plus gradient atlas
//!    into the backend.
//!
//! Output crosses into the backend as `&RenderBuffer` (defined one
//! level up so it sits at the frontend↔backend contract line).
//!
//! [`Host`]: crate::host::Host

pub(crate) mod cmd_buffer;
pub(crate) mod composer;
pub(crate) mod encoder;

use crate::common::frame_arena::{FrameArenaHandle, new_handle};
use crate::renderer::frontend::cmd_buffer::RenderCmdBuffer;
use crate::renderer::frontend::composer::Composer;
use crate::renderer::frontend::encoder::encode;
use crate::renderer::render_buffer::RenderBuffer;
use crate::ui::Ui;
use crate::ui::damage::Damage;

/// CPU paint stage: tree → encoded commands → composed buffer. Owns
/// every persistent allocation (the encoder's
/// [`RenderCmdBuffer`], the output `RenderBuffer` — which carries
/// the gradient atlas as a field — and the [`Composer`] with its
/// scratch). No GPU handles.
///
/// Owned by [`Host`](crate::host::Host) alongside the backend; the
/// host drives `Frontend::build` and hands the returned
/// `&RenderBuffer` straight to the backend.
pub(crate) struct Frontend {
    pub(crate) cmds: RenderCmdBuffer,
    pub(crate) composer: Composer,
    pub(crate) buffer: RenderBuffer,
    /// Shared frame arena (clone of `Host`'s canonical handle). Compose
    /// borrows it mutably to append polyline tessellation output and
    /// to read user-supplied mesh / polyline bytes.
    pub(crate) frame_arena: FrameArenaHandle,
}

impl Default for Frontend {
    /// Standalone frontend with a private frame arena. Production goes
    /// through [`Self::with_arena`] so the arena is shared across
    /// `Ui`, `Frontend`, and `WgpuBackend`.
    fn default() -> Self {
        Self::new(new_handle())
    }
}

impl Frontend {
    pub(crate) fn new(frame_arena: FrameArenaHandle) -> Self {
        Self {
            cmds: RenderCmdBuffer::default(),
            composer: Composer::default(),
            buffer: RenderBuffer::default(),
            frame_arena,
        }
    }

    /// Encode the tree into commands, compose them into the owned
    /// buffer, and return a borrow of the composed result.
    /// Disabled-dim and other paint-time theme constants are
    /// pre-resolved into `cascades` (`Cascade::rgb_mul`), so this
    /// stage reads everything it needs from the inputs without
    /// per-call theme threading.
    pub(crate) fn build(&mut self, ui: &Ui, damage: Damage) -> &RenderBuffer {
        encode(ui, damage, &mut self.cmds);
        let mut arena = self.frame_arena.borrow_mut();
        self.composer
            .compose(&self.cmds, &mut arena, ui.display, &mut self.buffer);
        &self.buffer
    }
}
