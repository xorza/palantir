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
use crate::image_registry::texture_limit::TextureLimit;
use crate::layout::Layout;
use crate::renderer::frontend::composer::Composer;
use crate::renderer::frontend::encoder::Encoder;
use crate::renderer::gradient_atlas::FlushedRows;
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

/// What the GPU half paints a built frame with: its draw list, and the gradient rows baked since
/// the last frame painted.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameOutput<'a> {
    pub(crate) buffer: &'a RenderBuffer,
    pub(crate) gradients: Option<FlushedRows<'a>>,
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
    pub(crate) fn new(device: Rc<SharedDevice>) -> Self {
        Self {
            encoder: Encoder::default(),
            composer: Composer::new(device),
            buffer: RenderBuffer::new(),
        }
    }

    /// The CPU side of a device attach, at startup and after a loss alike: start the gradient
    /// atlas over under the device's `texture_limit`.
    pub(crate) fn attach_device(&mut self, texture_limit: TextureLimit) {
        self.encoder.attach_device(texture_limit);
    }

    /// Rows the gradient atlas holds, the height a new backend's LUT texture starts at.
    pub(crate) const fn gradient_rows(&self) -> u32 {
        self.encoder.gradient_rows()
    }

    /// The last built frame as the GPU half paints it, which ends the gradient atlas's
    /// registration epoch: call it for a frame that paints, once.
    pub(crate) fn output(&mut self) -> FrameOutput<'_> {
        FrameOutput {
            buffer: &self.buffer,
            gradients: self.encoder.flush_gradients(),
        }
    }

    pub(crate) fn build(&mut self, scene: FrameScene<'_>, plan: BuildPlan<'_>) {
        tracy::zone!();
        let mut sink =
            self.composer
                .begin(scene.stamp, &scene.forest.record_store, &mut self.buffer);
        self.encoder.encode(&scene, plan, &mut sink);
        sink.finish();
        self.buffer.live_panes.clone_from(&scene.cascade.pane_keys);
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::image_registry::texture_limit::TextureLimit;
    use crate::renderer::frontend::BuildPlan;
    #[cfg(test)]
    use crate::renderer::frontend::Frontend;
    use crate::renderer::render_plan::RenderPlan;
    #[cfg(test)]
    use crate::renderer::shared_device::SharedDevice;
    use std::num::NonZeroU32;
    #[cfg(test)]
    use std::rc::Rc;

    /// No `wgpu::Device` to query; 8192 is `wgpu::Limits::default`'s `max_texture_dimension_2d`.
    pub(crate) const TEST_TEXTURE_LIMIT: TextureLimit =
        TextureLimit::from_device(NonZeroU32::new(8192).unwrap());

    /// A device attached at [`TEST_TEXTURE_LIMIT`].
    #[cfg(test)]
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

    #[cfg(test)]
    impl Frontend {
        pub(crate) fn for_test() -> Self {
            Self::new(test_device())
        }

        /// The growth ceiling of the gradient atlas, which the last device attach set.
        pub(crate) fn gradient_max_rows(&self) -> u32 {
            self.encoder.gradient_max_rows()
        }

        /// Leaves out `buffer`, the composed frame the backend reads.
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
