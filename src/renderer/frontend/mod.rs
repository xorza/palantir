//! Frontend (CPU) rendering pipeline.
//!
//! 1. [`encode`] — `&Tree` → [`RenderCmdBuffer`](cmd_buffer::RenderCmdBuffer)
//!    (logical-px). Pure free fn.
//! 2. [`Composer`] — `&RenderCmdBuffer` → `RenderBuffer` (physical-px
//!    quads + scissor groups). Owns the output + scratch; no GPU handles.
//! 3. [`Frontend`] (this struct) — orchestrates (1) + (2) and owns every
//!    persistent per-frame allocation. [`WindowRenderer`] calls [`Frontend::build`]
//!    once per frame and hands the composed buffer to the backend; the
//!    backend reads its own clone of `RenderCaches` (image registry +
//!    gradient atlas) for upload.
//!
//! Output crosses into the backend as `&RenderBuffer` (defined one
//! level up so it sits at the frontend↔backend contract line).
//!
//! [`WindowRenderer`]: crate::window_renderer::WindowRenderer

pub(crate) mod cmd_buffer;
pub(crate) mod composer;
pub(crate) mod encoder;

use crate::forest::frame_arena::FrameArena;
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
/// Owned by [`WindowRenderer`](crate::window_renderer::WindowRenderer) alongside the backend; the
/// host drives `Frontend::build` and hands the returned
/// `&RenderBuffer` straight to the backend.
pub struct Frontend {
    pub(crate) cmds: RenderCmdBuffer,
    pub(crate) composer: Composer,
    pub(crate) buffer: RenderBuffer,
    /// Shared frame arena (clone of `WindowRenderer`'s canonical handle). Compose
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
    pub(crate) fn build(&mut self, ui: &Ui, plan: RenderPlan) -> &RenderBuffer {
        // Two scoped borrows, not one held across both passes: encode
        // only reads the arena (shared borrow), compose appends polyline
        // tessellation (mutable). Keeping the mutable window to compose
        // alone means encode can't deadlock-panic against any other
        // shared reader, and the read/write split reads off the calls.
        {
            let arena = self.frame_arena.inner();
            encode(ui, &arena, plan, &mut self.cmds);
        }
        let mut arena = self.frame_arena.inner_mut();
        self.composer
            .compose(&self.cmds, &mut arena, ui.display, &mut self.buffer);
        &self.buffer
    }
}

#[cfg(any(test, feature = "internals"))]
pub mod test_support {
    #![allow(dead_code)]
    use crate::{renderer::frontend::*, ui::Ui};

    impl Frontend {
        /// `Frontend` with a private (disjoint-from-Ui) frame arena.
        pub fn for_test() -> Self {
            Self::new(FrameArena::default())
        }

        /// `Frontend` sharing `ui`'s frame arena. The arena holds per-frame
        /// shape/text/mesh payloads written during record; the encoder + composer
        /// read it on the same frame. Required for benches that want a
        /// full CPU-side frame including encode + compose.
        pub fn for_test_sharing(ui: &Ui) -> Self {
            Self::new(ui.frame_arena.clone())
        }

        /// Drive the full CPU-side frontend (encode + compose) against a
        /// just-recorded `Ui`. Bench / test reach-in for the otherwise
        /// `pub(crate)` `Frontend::build`. The output `RenderBuffer` is
        /// crate-private; the side effect (mutating `self.cmds`,
        /// `self.composer`, `self.buffer`) is what bench callers want
        /// timed, so the helper returns nothing.
        pub fn build_for_test(&mut self, ui: &Ui, plan: crate::renderer::frontend::RenderPlan) {
            let _ = self.build(ui, plan);
        }
    }
}
