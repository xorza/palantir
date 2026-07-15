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
//! [`WindowRenderer`]: crate::host::window_renderer::WindowRenderer

pub(crate) mod cmd_buffer;
pub(crate) mod composer;
pub(crate) mod encoder;

use crate::frame_arena::FrameArenaInner;
use crate::renderer::frontend::cmd_buffer::RenderCmdBuffer;
use crate::renderer::frontend::composer::Composer;
use crate::renderer::frontend::encoder::encode;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::owner::RenderOwnerId;
use crate::ui::Ui;
use crate::ui::frame_report::RenderPlan;

/// CPU paint stage: tree → encoded commands → composed buffer. Owns
/// every persistent allocation (the encoder's [`RenderCmdBuffer`],
/// the output `RenderBuffer`, and the [`Composer`] with its scratch).
/// No GPU handles; gradient atlas state lives on `RenderCaches`,
/// shared with the backend.
///
/// Owned by [`WindowRenderer`](crate::host::window_renderer::WindowRenderer);
/// the host builds into the staged [`Self::buffer`] before GPU submission.
#[derive(Debug)]
pub struct Frontend {
    cmds: RenderCmdBuffer,
    composer: Composer,
    pub(crate) buffer: RenderBuffer,
}

impl Frontend {
    /// `max_texture_dim` is the device's `max_texture_dimension_2d` (fixed for
    /// the device's lifetime) — the cap on `GpuView` target sizes, handed to
    /// the [`Composer`] which ceils each composited view into `frame_targets`.
    pub(crate) fn new(max_texture_dim: u32) -> Self {
        let owner = RenderOwnerId::reserve();
        Self {
            cmds: RenderCmdBuffer::default(),
            composer: Composer::new(max_texture_dim),
            buffer: RenderBuffer::new(owner),
        }
    }

    /// Encode → compose into the staged output buffer.
    #[profiling::function]
    pub(crate) fn build(&mut self, ui: &Ui, arena: &FrameArenaInner, plan: RenderPlan) {
        encode(ui, arena, plan, &mut self.cmds);
        self.composer
            .compose(&self.cmds, arena, ui.display, &mut self.buffer);
        // Stamp the frame clock for the backend's per-GpuView `dt` (not
        // derivable from `Display`, so it doesn't ride `start_frame`).
        self.buffer.time = ui.frame_runtime.time;
    }
}

#[cfg(any(test, feature = "internals"))]
pub mod test_support {
    #![allow(dead_code)]
    use crate::{renderer::frontend::*, ui::Ui};

    /// Baseline `max_texture_dimension_2d` for deviceless test/bench
    /// frontends — they have no `wgpu::Device` to query, and 8192 is the
    /// downlevel-default cap real adapters meet or exceed.
    const TEST_MAX_TEXTURE_DIM: u32 = 8192;

    impl Frontend {
        /// Deviceless frontend for tests and benchmarks.
        pub fn for_test() -> Self {
            Self::new(TEST_MAX_TEXTURE_DIM)
        }

        /// Deviceless frontend paired with `ui` by the caller at build time.
        pub fn for_test_sharing(_ui: &Ui) -> Self {
            Self::new(TEST_MAX_TEXTURE_DIM)
        }

        /// Drive the full CPU-side frontend (encode + compose) against a
        /// just-recorded `Ui`. Bench / test reach-in for the otherwise
        /// `pub(crate)` `Frontend::build`. The output `RenderBuffer` is
        /// crate-private; the side effect (mutating `self.cmds`,
        /// `self.composer`, `self.buffer`) is what bench callers want
        /// timed, so the helper returns nothing.
        pub fn build_for_test(&mut self, ui: &Ui, plan: RenderPlan) {
            let arena = ui.frame_arena.inner();
            self.build(ui, &arena, plan);
        }
    }
}
