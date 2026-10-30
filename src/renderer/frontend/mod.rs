//! CPU paint: the [`Encoder`] walks the tree into a [`Composer`] session that fills a
//! `RenderBuffer` (physical-px quads and scissor groups). No GPU handles.

#[cfg(feature = "internals")]
pub(crate) mod bench;
pub(crate) mod composer;
pub(crate) mod encoder;
pub(crate) mod paint_sink;
pub(crate) mod payload;

use crate::cascade::Cascade;
use crate::cascade::pane_row::PaneRow;
use crate::common::tracy;
use crate::display::frame_stamp::FrameStamp;
use crate::layout::Layout;
use crate::renderer::frontend::composer::Composer;
use crate::renderer::frontend::encoder::Encoder;
use crate::renderer::gpu_paint::gpu_views::GpuViews;
use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
use crate::renderer::pane_captures::PaneTicket;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_plan::RenderPlan;
use crate::renderer::shared_device::SharedDevice;
use crate::scene::forest::Forest;
use std::rc::Rc;

/// What a frame builds: the plan it paints by, and the ticket of each pane of its scene, planes
/// and panes in paint order.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BuildPlan<'a> {
    pub(crate) render: RenderPlan,
    pub(crate) panes: &'a [Option<PaneTicket>],
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameScene<'a> {
    pub(crate) forest: &'a Forest,
    pub(crate) layout: &'a Layout,
    pub(crate) cascade: &'a Cascade,
    pub(crate) gpu_views: &'a GpuViews,
    /// The display and time; the time drives backend `GpuView` frame deltas.
    pub(crate) stamp: FrameStamp,
}

impl FrameScene<'_> {
    /// The scene's panes, planes and panes in paint order.
    pub(crate) fn panes(&self) -> impl Iterator<Item = &PaneRow> {
        self.forest
            .trees
            .iter_paint_order()
            .flat_map(|(plane, _)| &self.cascade.planes[plane].panes)
    }
}

/// CPU paint stage: tree to composed buffer, reused serially across window drivers.
#[derive(Debug)]
pub(crate) struct Frontend {
    encoder: Encoder,
    composer: Composer,
    pub(crate) buffer: RenderBuffer,
}

impl Frontend {
    /// `device`'s texture limit caps `GpuView` target sizes; the [`Composer`] downsamples larger
    /// views.
    pub(crate) fn new(device: Rc<SharedDevice>, gradient_atlas: SharedGradientAtlas) -> Self {
        Self {
            encoder: Encoder::new(gradient_atlas),
            composer: Composer::new(device),
            buffer: RenderBuffer::new(),
        }
    }

    pub(crate) fn build(&mut self, scene: FrameScene<'_>, plan: BuildPlan<'_>) {
        tracy::zone!();
        let mut sink =
            self.composer
                .begin(scene.stamp, &scene.forest.record_store, &mut self.buffer);
        self.encoder.encode(&scene, plan, &mut sink);
        drop(sink);
        // Written after the session drops, since the composer's clear fold
        // discards scene columns mid-compose and this is not one.
        scene
            .gpu_views
            .collect_live_targets(&mut self.buffer.live_targets);
        self.buffer.live_panes.clear();
        for (plane, _) in scene.forest.trees.iter_paint_order() {
            self.buffer.live_panes.extend(
                scene.cascade.planes[plane]
                    .panes
                    .iter()
                    .map(|pane| pane.key),
            );
        }
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::renderer::frontend::{BuildPlan, Frontend};
    use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
    use crate::renderer::render_plan::RenderPlan;
    use crate::renderer::shared_device::SharedDevice;
    use crate::renderer::texture_limit::TextureLimit;
    use std::num::NonZeroU32;
    use std::rc::Rc;

    /// No `wgpu::Device` to query; 8192 is `wgpu::Limits::default`'s `max_texture_dimension_2d`.
    pub(crate) const TEST_TEXTURE_LIMIT: TextureLimit =
        TextureLimit::from_device(NonZeroU32::new(8192).unwrap());

    /// A device attached at [`TEST_TEXTURE_LIMIT`].
    pub(crate) fn test_device() -> Rc<SharedDevice> {
        Rc::new(SharedDevice::attached(TEST_TEXTURE_LIMIT))
    }

    impl BuildPlan<'static> {
        /// A build of a scene that holds no pane, as a test or a bench drives one; the encoder
        /// asserts a ticket for each pane.
        pub(crate) const fn without_panes(render: RenderPlan) -> Self {
            Self { render, panes: &[] }
        }
    }

    impl Frontend {
        pub(crate) fn for_test() -> Self {
            Self::new(test_device(), SharedGradientAtlas::default())
        }

        /// Leaves out `buffer`, the composed frame the backend reads.
        #[cfg(test)]
        pub(crate) fn scratch_bytes(&self) -> usize {
            let Self {
                encoder,
                composer,
                buffer: _,
            } = self;
            encoder.scratch_bytes() + composer.scratch_bytes()
        }
    }
}
