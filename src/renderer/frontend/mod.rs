//! CPU paint: the [`Encoder`] walks the tree into a [`Composer`] session that fills a
//! `RenderBuffer` (physical-px quads and scissor groups). No GPU handles.

#[cfg(feature = "internals")]
pub(crate) mod bench;
pub(crate) mod composer;
pub(crate) mod encoder;
pub(crate) mod paint_sink;
pub(crate) mod payload;

use crate::cascade::Cascade;
use crate::cascade::paint::PaintRows;
use crate::cascade::plane_backdrop::PlaneBackdrop;
use crate::common::tracy;
use crate::display::frame_stamp::FrameStamp;
use crate::image_registry::texture_limit::TextureLimit;
use crate::layout::Layout;
use crate::renderer::backdrop_captures::BackdropTicket;
use crate::renderer::cpu_gradient_atlas::FlushedRows;
use crate::renderer::frontend::composer::Composer;
use crate::renderer::frontend::encoder::Encoder;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_plan::RenderPlan;
use crate::scene::forest::Forest;

/// What a frame builds: the plan it paints by, and the ticket of each backdrop of its scene, planes
/// and backdrops in paint order.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BuildPlan<'a> {
    pub(crate) render: RenderPlan,
    pub(crate) backdrops: &'a [Option<BackdropTicket>],
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
    /// How many paint rows draw on the surface, over every plane: a child marker draws nothing, and
    /// a frame culls a row off the surface however it repaints.
    pub(crate) fn visible_row_count(&self) -> usize {
        let surface = self.stamp.display.logical_rect();
        self.forest
            .trees
            .iter_paint_order()
            .map(|(plane, _)| {
                self.cascade.planes[plane]
                    .paint_arena
                    .rows
                    .screens()
                    .filter(|screen| screen.intersects(surface))
                    .count()
            })
            .sum()
    }

    /// The share of the surface the scene's backdrop footprints hold, each backdrop's counted
    /// alone, so backdrops that overlap can read past one.
    pub(crate) fn backdrop_share(&self) -> f32 {
        let surface = self.stamp.display.logical_rect();
        let area: f32 = self
            .backdrops()
            .map(|backdrop| backdrop.footprint.clamp_to(surface).area())
            .sum();
        area / surface.area()
    }

    /// The scene's backdrops, planes and backdrops in paint order.
    pub(crate) fn backdrops(&self) -> impl Iterator<Item = &PlaneBackdrop> {
        self.forest
            .trees
            .iter_paint_order()
            .flat_map(|(plane, _)| &self.cascade.planes[plane].backdrops)
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
    /// A frontend with no device yet, whose [`Composer`] downsamples no `GpuView`.
    pub(crate) fn new() -> Self {
        Self {
            encoder: Encoder::default(),
            composer: Composer::default(),
            buffer: RenderBuffer::new(),
        }
    }

    /// The CPU side of a device attach, at startup and after a loss alike: start the gradient
    /// atlas over under the device's `texture_limit`, which also caps `GpuView` target sizes.
    pub(crate) fn attach_device(&mut self, texture_limit: TextureLimit) {
        self.encoder.attach_device(texture_limit);
        self.composer.attach_device(texture_limit);
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
        self.buffer
            .live_backdrops
            .clone_from(&scene.cascade.backdrop_keys);
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::image_registry::texture_limit::TextureLimit;
    use crate::renderer::cpu_gradient_atlas::CpuGradientAtlas;
    use crate::renderer::frontend::BuildPlan;
    use crate::renderer::frontend::Frontend;
    use crate::renderer::render_plan::RenderPlan;
    use std::num::NonZeroU32;

    /// No `wgpu::Device` to query; 8192 is `wgpu::Limits::default`'s `max_texture_dimension_2d`.
    pub(crate) const TEST_TEXTURE_LIMIT: TextureLimit =
        TextureLimit::from_device(NonZeroU32::new(8192).unwrap());

    impl BuildPlan<'static> {
        /// A build of a scene that holds no backdrop, as a test or a bench drives one; the encoder
        /// asserts a ticket for each backdrop.
        pub(crate) const fn without_backdrops(render: RenderPlan) -> Self {
            Self {
                render,
                backdrops: &[],
            }
        }
    }

    impl Frontend {
        /// The gradient atlas the frame's ramps register in.
        pub(crate) const fn gradient_atlas(&self) -> &CpuGradientAtlas {
            self.encoder.gradient_atlas()
        }
    }

    #[cfg(test)]
    impl Frontend {
        /// A frontend attached at [`TEST_TEXTURE_LIMIT`].
        pub(crate) fn for_test() -> Self {
            let mut frontend = Self::new();
            frontend.attach_device(TEST_TEXTURE_LIMIT);
            frontend
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
