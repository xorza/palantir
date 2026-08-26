//! Frontend (CPU) rendering pipeline.
//!
//! 1. [`Encoder`] — walks `&Tree` and paints logical-px operations into
//!    a [`PaintSink`](paint_sink::PaintSink). Owns the encode scratch.
//! 2. [`Composer`] — the production sink: scales, snaps, and groups each
//!    operation into a `RenderBuffer` (physical-px quads + scissor
//!    groups). Owns the compose scratch; the buffer it fills is lent to it
//!    per pass by (3). No GPU handles.
//! 3. [`Frontend`] (this struct) — orchestrates (1) + (2) and owns every
//!    persistent per-frame allocation. A host shares one frontend serially
//!    across its windows: `WindowDriver` calls [`Frontend::build`] once per
//!    painted frame and hands the composed buffer to the backend. The frontend
//!    and backend hold capability-specific clones of the shared gradient atlas
//!    and image registry.
//!
//! Output crosses into the backend as `&RenderBuffer` (defined one
//! level up so it sits at the frontend↔backend contract line).

#[cfg(feature = "bench")]
pub(crate) mod bench;
#[cfg(any(test, feature = "bench"))]
pub(crate) mod capture;
pub(crate) mod composer;
pub(crate) mod encoder;
pub(crate) mod paint_sink;
pub(crate) mod payload;

use std::cell::Ref;
use std::time::Duration;

use crate::common::tracy;
use crate::display::Display;
use crate::layout::Layout;
use crate::primitives::widget_id::WidgetIdMap;
use crate::renderer::frontend::composer::Composer;
use crate::renderer::frontend::encoder::Encoder;
use crate::renderer::gpu_paint::gpu_view_entry::GpuViewEntry;
use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_plan::RenderPlan;
use crate::scene::cascade::Cascade;
use crate::scene::forest::Forest;
use crate::scene::record_store::record_payloads::RecordPayloads;

/// Frozen inputs consumed by the CPU renderer for one frame.
#[derive(Debug)]
pub(crate) struct FrameScene<'a> {
    pub(crate) forest: &'a Forest,
    pub(crate) layout: &'a Layout,
    pub(crate) cascade: &'a Cascade,
    /// Keeps the record-store read lease alive through encode and compose.
    pub(crate) payloads: Ref<'a, RecordPayloads>,
    pub(crate) gpu_views: &'a WidgetIdMap<GpuViewEntry>,
    pub(crate) display: Display,
    /// Drives backend `GpuView` frame deltas and is not derivable from `Display`.
    pub(crate) time: Duration,
}

/// CPU paint stage: tree → encoded commands → composed buffer. Owns
/// every persistent allocation (the [`Encoder`], output `RenderBuffer`,
/// and the [`Composer`] with its scratch).
/// No GPU handles; its gradient-atlas handle shares state with the backend.
///
/// Owned once by the host and reused serially across its window drivers. The
/// active driver builds into the staged [`Self::buffer`] immediately before GPU
/// submission.
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
    pub(crate) fn new(max_texture_dim: u32, gradient_atlas: SharedGradientAtlas) -> Self {
        Self {
            encoder: Encoder::new(gradient_atlas),
            composer: Composer::new(max_texture_dim),
            buffer: RenderBuffer::new(),
        }
    }

    /// Encode straight into the composer, filling the staged output
    /// buffer. One pass: the encoder's paint calls land in a live
    /// [`ComposeSession`](composer::session::ComposeSession) rather than an
    /// intermediate command stream, so
    /// nothing is serialized only to be read back a line later.
    pub(crate) fn build(&mut self, scene: FrameScene<'_>, plan: RenderPlan) {
        tracy::zone!();
        let mut sink =
            self.composer
                .begin(scene.display, scene.time, &scene.payloads, &mut self.buffer);
        self.encoder.encode(&scene, plan, &mut sink);
        // Dropping the session closes the trailing batch and group;
        // explicit because it also releases the `buffer` borrow.
        drop(sink);
        // The retention roster, filled from the live view map rather than
        // from what composed: `buffer.frame_targets` holds only the views
        // this frame *paints*, and an unchanged view is culled out of it by
        // the damage diff. Keyed on that alone, the backend could not tell
        // "unchanged" from "gone" and would free a live view's target.
        // Written after the session drops, since the composer's clear fold
        // discards scene columns mid-compose and this is not one.
        let live = &mut self.buffer.live_targets;
        live.clear();
        live.reserve_exact(scene.gpu_views.len());
        live.extend(scene.gpu_views.values().map(|view| view.texture_id));
    }
}

#[cfg(any(test, feature = "bench"))]
pub(crate) mod test_support {
    use crate::renderer::frontend::Frontend;

    /// Baseline `max_texture_dimension_2d` for deviceless test/bench
    /// frontends — they have no `wgpu::Device` to query, and 8192 is the
    /// downlevel-default cap real adapters meet or exceed.
    const TEST_MAX_TEXTURE_DIM: u32 = 8192;

    impl Frontend {
        /// Deviceless frontend for tests and benchmarks.
        pub(crate) fn for_test() -> Self {
            Self::new(TEST_MAX_TEXTURE_DIM, Default::default())
        }
    }
}
