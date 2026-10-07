//! CPU paint: the [`Encoder`] walks the tree into a [`Composer`] session that fills a
//! `RenderBuffer` (physical-px quads and scissor groups). No GPU handles.

#[cfg(feature = "bench")]
pub(crate) mod bench;
pub(crate) mod composer;
pub(crate) mod encoder;
pub(crate) mod paint_sink;
pub(crate) mod payload;

use std::time::Duration;

use crate::cascade::Cascade;
use crate::common::tracy;
use crate::display::Display;
use crate::layout::Layout;
use crate::renderer::frontend::composer::Composer;
use crate::renderer::frontend::encoder::Encoder;
use crate::renderer::gpu_paint::gpu_views::GpuViews;
use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_plan::RenderPlan;
use crate::scene::forest::Forest;
use std::num::NonZeroU32;

#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameScene<'a> {
    pub(crate) forest: &'a Forest,
    pub(crate) layout: &'a Layout,
    pub(crate) cascade: &'a Cascade,
    pub(crate) gpu_views: &'a GpuViews,
    pub(crate) display: Display,
    /// Drives backend `GpuView` frame deltas and is not derivable from `Display`.
    pub(crate) time: Duration,
}

/// CPU paint stage: tree to composed buffer, reused serially across window drivers.
#[derive(Debug)]
pub(crate) struct Frontend {
    encoder: Encoder,
    composer: Composer,
    pub(crate) buffer: RenderBuffer,
}

impl Frontend {
    /// `max_texture_dim` caps `GpuView` target sizes; the [`Composer`] downsamples larger views.
    pub(crate) fn new(max_texture_dim: NonZeroU32, gradient_atlas: SharedGradientAtlas) -> Self {
        Self {
            encoder: Encoder::new(gradient_atlas),
            composer: Composer::new(max_texture_dim),
            buffer: RenderBuffer::new(),
        }
    }

    pub(crate) fn build(&mut self, scene: FrameScene<'_>, plan: RenderPlan) {
        tracy::zone!();
        let mut sink = self.composer.begin(
            scene.display,
            scene.time,
            &scene.forest.record_store,
            &mut self.buffer,
        );
        self.encoder.encode(&scene, plan, &mut sink);
        drop(sink);
        // Written after the session drops, since the composer's clear fold
        // discards scene columns mid-compose and this is not one.
        scene
            .gpu_views
            .collect_live_targets(&mut self.buffer.live_targets);
    }
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
    use crate::renderer::frontend::Frontend;
    use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
    use std::num::NonZeroU32;

    /// No `wgpu::Device` to query; 8192 is the downlevel-default cap.
    pub(crate) const TEST_MAX_TEXTURE_DIM: NonZeroU32 = NonZeroU32::new(8192).unwrap();

    impl Frontend {
        pub(crate) fn for_test() -> Self {
            Self::new(TEST_MAX_TEXTURE_DIM, SharedGradientAtlas::default())
        }
    }
}
