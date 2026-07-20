//! Frontend (CPU) rendering pipeline.
//!
//! 1. [`Encoder`] — `&Tree` → [`RenderCmdBuffer`](cmd_buffer::RenderCmdBuffer)
//!    (logical-px). Owns the command output and encode scratch.
//! 2. [`Composer`] — `&RenderCmdBuffer` → `RenderBuffer` (physical-px
//!    quads + scissor groups). Owns the output + scratch; no GPU handles.
//! 3. [`Frontend`] (this struct) — orchestrates (1) + (2) and owns every
//!    persistent per-frame allocation. [`WindowDriver`] calls [`Frontend::build`]
//!    once per frame and hands the composed buffer to the backend; the
//!    backend reads its own clone of `RenderAssets` (image registry +
//!    gradient atlas) for upload.
//!
//! Output crosses into the backend as `&RenderBuffer` (defined one
//! level up so it sits at the frontend↔backend contract line).
//!
//! [`WindowDriver`]: crate::host::window_driver::WindowDriver

pub(crate) mod cmd_buffer;
pub(crate) mod composer;
pub(crate) mod encoder;

use std::cell::Ref;
use std::time::Duration;

use crate::display::Display;
use crate::layout::Layout;
use crate::primitives::widget_id::WidgetIdMap;
use crate::renderer::frontend::composer::Composer;
use crate::renderer::frontend::encoder::Encoder;
use crate::renderer::gpu_view::GpuViewEntry;
use crate::renderer::gradient_atlas::handle::GradientAtlas;
use crate::renderer::plan::RenderPlan;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::owner::RenderOwnerId;
use crate::scene::Forest;
use crate::scene::cascade::Cascades;
use crate::scene::record_store::RecordPayloads;
use crate::text::TextShaper;

/// Frozen inputs consumed by the CPU renderer for one frame.
pub(crate) struct FrameScene<'a> {
    pub(crate) forest: &'a Forest,
    pub(crate) layout: &'a Layout,
    pub(crate) cascades: &'a Cascades,
    /// Keeps the record-store read lease alive through encode and compose.
    pub(crate) payloads: Ref<'a, RecordPayloads>,
    pub(crate) text: &'a TextShaper,
    pub(crate) gradient_atlas: &'a GradientAtlas,
    pub(crate) gpu_views: &'a WidgetIdMap<GpuViewEntry>,
    pub(crate) display: Display,
    /// Drives backend `GpuView` frame deltas and is not derivable from `Display`.
    pub(crate) time: Duration,
}

impl std::fmt::Debug for FrameScene<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameScene")
            .field("display", &self.display)
            .field("time", &self.time)
            .finish_non_exhaustive()
    }
}

/// CPU paint stage: tree → encoded commands → composed buffer. Owns
/// every persistent allocation (the [`Encoder`], output `RenderBuffer`,
/// and the [`Composer`] with its scratch).
/// No GPU handles; gradient atlas state lives on `RenderAssets`,
/// shared with the backend.
///
/// Owned by [`WindowDriver`](crate::host::window_driver::WindowDriver);
/// the host builds into the staged [`Self::buffer`] before GPU submission.
#[derive(Debug)]
pub(crate) struct Frontend {
    encoder: Encoder,
    composer: Composer,
    pub(crate) buffer: RenderBuffer,
}

impl Frontend {
    /// `max_texture_dim` is the device's `max_texture_dimension_2d` (fixed for
    /// the device's lifetime) — the cap on `GpuView` target sizes, handed to
    /// the [`Composer`] which uniformly downsamples oversized composited views.
    pub(crate) fn new(max_texture_dim: u32) -> Self {
        let owner = RenderOwnerId::reserve();
        Self {
            encoder: Encoder::default(),
            composer: Composer::new(max_texture_dim),
            buffer: RenderBuffer::new(owner),
        }
    }

    /// Encode → compose into the staged output buffer.
    #[profiling::function]
    pub(crate) fn build(&mut self, scene: FrameScene<'_>, plan: RenderPlan) {
        let cmds = self.encoder.encode(&scene, plan);
        self.composer
            .compose(cmds, &scene.payloads, scene.display, &mut self.buffer);
        self.buffer.time = scene.time;
    }
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod test_support {
    #![allow(dead_code)]
    use crate::renderer::frontend::Frontend;

    /// Baseline `max_texture_dimension_2d` for deviceless test/bench
    /// frontends — they have no `wgpu::Device` to query, and 8192 is the
    /// downlevel-default cap real adapters meet or exceed.
    const TEST_MAX_TEXTURE_DIM: u32 = 8192;

    impl Frontend {
        /// Deviceless frontend for tests and benchmarks.
        pub(crate) fn for_test() -> Self {
            Self::new(TEST_MAX_TEXTURE_DIM)
        }
    }
}
