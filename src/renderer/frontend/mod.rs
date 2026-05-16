//! Frontend (CPU) rendering pipeline.
//!
//! 1. [`encode`] — `&Tree` → [`RenderCmdBuffer`](cmd_buffer::RenderCmdBuffer)
//!    (logical-px). Pure free fn.
//! 2. [`Composer`] — `&RenderCmdBuffer` → `RenderBuffer` (physical-px
//!    quads + scissor groups). Owns the output + scratch; no GPU handles.
//! 3. [`Frontend`] (this struct) — orchestrates (1) + (2) and owns every
//!    persistent per-frame allocation. [`Host`] calls [`Frontend::build`]
//!    once per frame and hands the composed buffer to the backend; the
//!    backend reads its own clone of `RenderCaches` (image registry +
//!    gradient atlas) for upload.
//!
//! Output crosses into the backend as `&RenderBuffer` (defined one
//! level up so it sits at the frontend↔backend contract line).
//!
//! [`Host`]: crate::host::Host

pub(crate) mod cmd_buffer;
pub(crate) mod composer;
pub mod encoder;

use crate::common::frame_arena::FrameArena;
use crate::renderer::frontend::cmd_buffer::RenderCmdBuffer;
use crate::renderer::frontend::composer::Composer;
use crate::renderer::frontend::encoder::encode;
use crate::renderer::render_buffer::RenderBuffer;
use crate::ui::Ui;
use crate::ui::frame_report::RenderPlan;

/// CPU paint stage: tree → encoded commands → composed buffer. Owns
/// every persistent allocation (the encoder's [`RenderCmdBuffer`],
/// the output `RenderBuffer`, and the [`Composer`] with its scratch).
/// No GPU handles; gradient atlas state lives on `RenderCaches`,
/// shared with the backend.
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
    pub(crate) frame_arena: FrameArena,
}

impl Frontend {
    pub(crate) fn new(frame_arena: FrameArena) -> Self {
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
    pub(crate) fn build<T>(&mut self, ui: &Ui<T>, plan: RenderPlan) -> &RenderBuffer {
        let mut arena = self.frame_arena.inner_mut();
        encode(ui, &arena, plan, &mut self.cmds);
        self.composer
            .compose(&self.cmds, &mut arena, ui.display, &mut self.buffer);
        &self.buffer
    }
}

#[cfg(any(test, feature = "internals"))]
pub mod test_support {
    #![allow(dead_code)]
    use super::*;

    impl Frontend {
        /// `Frontend` with a private (disjoint-from-Ui) frame arena.
        pub fn for_test() -> Self {
            Self::new(FrameArena::default())
        }
    }
}
