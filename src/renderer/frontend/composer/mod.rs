//! Turning a frame of paint calls into the buffer the backend draws.
//!
//! [`Composer`] owns the scratch a compose pass is built in; a
//! [`ComposeSession`] is one pass, holding that scratch together with the
//! buffer it fills, and [`geometry`] is the arithmetic both are cut with.

use crate::display::frame_stamp::FrameStamp;
use crate::image_registry::texture_limit::TextureLimit;
use crate::renderer::frontend::composer::clip_stack::ClipStack;
use crate::renderer::frontend::composer::paint_order::PaintOrder;
use crate::renderer::frontend::composer::polyline_scratch::PolylineScratch;
use crate::renderer::frontend::composer::session::ComposeSession;
use crate::renderer::frontend::composer::transform_stack::TransformStack;
use crate::renderer::render_buffer::RenderBuffer;
use crate::shape::record_store::RecordStore;

mod backdrop_break;
#[cfg(feature = "internals")]
pub(crate) mod bench;
mod clip_stack;
mod curve_run;
mod geometry;
mod higher_kind_rects;
mod lowering;
mod occlusion_pruner;
mod packed_quad;
mod paint_order;
mod physical_map;
mod polyline_scratch;
mod quad_runs;
mod rect_grid;
pub(crate) mod session;
mod stroke_bbox;
mod template_shape;
mod tile_index;
mod transform_stack;

/// The retained half of the CPU compose engine: buffers and stacks kept across
/// frames so steady-state rendering is alloc-free, plus the one device
/// constant a pass needs.
///
/// The pass itself is [`ComposeSession`], which holds this beside the
/// `RenderBuffer` being filled; the split is by lifetime. Nothing here takes
/// an output buffer. [`Frontend`](crate::renderer::frontend::Frontend)
/// orchestrates encode and compose.
#[derive(Debug, Default)]
pub(crate) struct Composer {
    /// The nested clips the walk has open: scissor plus rounded-clip chain per level.
    clip: ClipStack,
    /// The walk transform: live product plus the ancestors a pop restores.
    transform: TransformStack,
    polyline: PolylineScratch,
    /// Which draws may reorder within a group, and where groups and text batches close.
    order: PaintOrder,
    /// The device ceiling: the composer downsamples any composited `GpuView` whose physical rect
    /// exceeds it.
    texture_limit: TextureLimit,
}

impl Composer {
    /// The composer's side of a device attach: downsample under the device's `texture_limit`.
    pub(crate) const fn attach_device(&mut self, texture_limit: TextureLimit) {
        self.texture_limit = texture_limit;
    }

    /// Open a compose session over `out`: stamp the display, reset scratch and
    /// walk state, and hand back the sink. [`ComposeSession::finish`] closes the
    /// trailing batch and group.
    pub(crate) fn begin<'a>(
        &'a mut self,
        stamp: FrameStamp,
        store: &'a RecordStore,
        out: &'a mut RenderBuffer,
    ) -> ComposeSession<'a> {
        out.start_frame(stamp);

        self.order.start_frame(stamp.display.physical);
        self.clip.clear();
        self.transform.clear(stamp.display.scale_factor());

        ComposeSession {
            composer: self,
            store,
            out,
        }
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    //! Replay driver for the composer tests and the compose bench.

    use crate::internals::paint_capture::PaintCapture;
    #[cfg(test)]
    use crate::renderer::frontend::composer::Composer;
    use crate::renderer::frontend::composer::session::ComposeSession;

    impl ComposeSession<'_> {
        /// Replay a recorded paint stream into this session and finish it, so
        /// tests and benches can drive the composer from a stream captured once.
        pub(crate) fn replay_from(mut self, recorded: &PaintCapture) {
            recorded.replay(&mut self);
            self.finish();
        }
    }

    #[cfg(test)]
    impl Composer {
        pub(crate) fn scratch_bytes(&self) -> usize {
            let Self {
                clip,
                transform,
                polyline,
                order,
                texture_limit: _,
            } = self;
            clip.scratch_bytes()
                + transform.scratch_bytes()
                + polyline.scratch_bytes()
                + order.scratch_bytes()
        }
    }
}

#[cfg(test)]
mod tests;
