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
use crate::renderer::frontend::composer::Composer;
use crate::renderer::frontend::encoder::Encoder;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_plan::RenderPlan;
use crate::renderer::row_table::RowTable;
use crate::scene::forest::Forest;

/// What a frame builds: the plan it paints by, the ticket of each backdrop of its scene, planes
/// and backdrops in paint order, and the row table its gradients take rows in.
#[derive(Debug)]
pub(crate) struct BuildPlan<'a> {
    pub(crate) render: RenderPlan,
    pub(crate) backdrops: &'a [Option<BackdropTicket>],
    pub(crate) rows: &'a mut RowTable,
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

    /// The CPU side of a device attach, at startup and after a loss alike: the device's
    /// `texture_limit` caps `GpuView` target sizes.
    pub(crate) const fn attach_device(&mut self, texture_limit: TextureLimit) {
        self.composer.attach_device(texture_limit);
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
    use crate::renderer::frontend::BuildPlan;
    #[cfg(test)]
    use crate::renderer::frontend::{FrameScene, Frontend};
    use crate::renderer::render_plan::RenderPlan;
    use crate::renderer::row_table::RowTable;
    use std::num::NonZeroU32;

    /// No `wgpu::Device` to query; 8192 is `wgpu::Limits::default`'s `max_texture_dimension_2d`.
    pub(crate) const TEST_TEXTURE_LIMIT: TextureLimit =
        TextureLimit::from_device(NonZeroU32::new(8192).unwrap());

    impl<'a> BuildPlan<'a> {
        /// A build of a scene that holds no backdrop, its gradients in `rows`, as a test or a bench
        /// drives one; the encoder asserts a ticket for each backdrop.
        pub(crate) const fn without_backdrops(render: RenderPlan, rows: &'a mut RowTable) -> Self {
            Self {
                render,
                backdrops: &[],
                rows,
            }
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

        /// Build `scene` by `render`, which holds no backdrop, its gradients in a row table of
        /// their own, as a test that reads the draw list alone builds it.
        pub(crate) fn build_alone(&mut self, scene: FrameScene<'_>, render: RenderPlan) {
            self.build(
                scene,
                BuildPlan::without_backdrops(render, &mut RowTable::default()),
            );
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
