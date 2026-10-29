//! Turning a frame of paint calls into the buffer the backend draws.
//!
//! [`Composer`] owns the scratch a compose pass is built in; a
//! [`ComposeSession`] is one pass, holding that scratch together with the
//! buffer it fills, and [`geometry`] is the arithmetic both are cut with.

use crate::display::frame_stamp::FrameStamp;
use crate::renderer::frontend::composer::clip_stack::ClipStack;
use crate::renderer::frontend::composer::paint_order::PaintOrder;
use crate::renderer::frontend::composer::polyline_scratch::PolylineScratch;
use crate::renderer::frontend::composer::session::ComposeSession;
use crate::renderer::frontend::composer::transform_stack::TransformStack;
use crate::renderer::render_buffer::RenderBuffer;
use crate::shape::record_store::RecordStore;
use std::num::NonZeroU32;

#[cfg(feature = "internals")]
pub(crate) mod bench;
mod clip_stack;
#[cfg(feature = "internals")]
pub(crate) mod compose_rig;
mod curve_run;
mod geometry;
mod higher_kind_rects;
mod lowering;
mod occlusion_pruner;
mod packed_quad;
mod paint_order;
mod pane_break;
mod physical_map;
mod polyline_scratch;
mod rect_grid;
pub(crate) mod session;
mod stroke_bbox;
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
#[derive(Debug)]
pub(crate) struct Composer {
    /// The nested clips the walk has open: scissor plus rounded-mask chain per level.
    clip: ClipStack,
    /// The walk transform: live product plus the ancestors a pop restores.
    transform: TransformStack,
    polyline: PolylineScratch,
    /// Which draws may reorder within a group, and where groups and text batches close.
    order: PaintOrder,
    /// Device `max_texture_dimension_2d`: the composer downsamples any
    /// composited `GpuView` whose physical rect exceeds it. Fixed per device,
    /// so it rides the ctor.
    max_texture_dim: NonZeroU32,
}

impl Composer {
    /// New composer capped at the device's `max_texture_dimension_2d`, all
    /// scratch empty.
    pub(crate) fn new(max_texture_dim: NonZeroU32) -> Self {
        Self {
            clip: ClipStack::default(),
            transform: TransformStack::default(),
            polyline: PolylineScratch::default(),
            order: PaintOrder::default(),
            max_texture_dim,
        }
    }

    /// Open a compose session over `out`: stamp the display, reset scratch and
    /// walk state, and hand back the sink. Dropping the [`ComposeSession`]
    /// closes the trailing batch and group.
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
        /// Replay a recorded paint stream into this session, closing it, so
        /// tests and benches can drive the composer from a stream captured once.
        pub(crate) fn replay_from(mut self, recorded: &PaintCapture) {
            recorded.replay(&mut self);
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
                max_texture_dim: _,
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
