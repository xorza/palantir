//! The GPU time of a frame's work, published into [`GpuPassStats`]: one entity over the tracer
//! the device takes, so the backend records the same calls on every platform.
//!
//! - [`QueryTracer`]: `wgpu` timestamp and pipeline-statistics queries, on a device that writes
//!   the timestamps the timing needs.
//! - [`WallClockTracer`]: each frame from its submit to the queue's work-done callback, on the CPU
//!   clock, on a device that does not.
//!
//! A step a tracer cannot take does nothing, and what it cannot measure publishes as absent.
//! A frame publishes late: the query tracer's two frames after it, the wall clock's once the
//! queue said it is done. Both ask the queue to call back once a frame's work is done, which
//! wgpu boxes, an allocation per frame a frame pays only while it collects GPU statistics.
//!
//! # Decisions
//!
//! - **One tracer per device, picked by what it writes.** A frame takes the query tracer on a
//!   device with `TIMESTAMP_QUERY`, as it times its main pass by the pass's own timestamps; a run
//!   of passes a bench times takes it with `TIMESTAMP_QUERY_INSIDE_ENCODERS`, as it times them
//!   between passes. Otherwise both take the wall clock. As Flutter's Impeller keeps one GPU
//!   tracer per backend behind one role. An enum, as the set is closed. Refused: a branch on the
//!   platform; a trait object, for a closed set.
//! - **A frame's queries resolve after the queue said it is done.** On an M5 (macOS 27) a
//!   render pass's end timestamp, resolved in the command buffer that wrote it or in one
//!   submitted at once after, reads as zero or as the frame before's; resolved after the
//!   work-done callback, it reads the pass's 22 ms against 22.6 ms of wall time (wgpu #9414
//!   reports the same race on macOS 26). So every backend resolves a frame in a later one, two
//!   frames late. Refused: the wall clock for Metal frames, whose latency reads tens of µs too
//!   long; Metal's command buffer times, which wgpu 30 cannot hand out, as its `as_hal` encoder
//!   refuses to mix with wgpu encoding; empty passes bracketing the work, which Metal drops
//!   with their samples (Tracy PR 793).

mod query_tracer;
mod wall_clock_tracer;

use crate::diagnostics::gpu_pass_stats::{BatchKind, GpuPassStats};
use crate::gpu::device::gpu::Gpu;
use crate::gpu::frame::gpu_timings::query_tracer::QueryTracer;
use crate::gpu::frame::gpu_timings::wall_clock_tracer::WallClockTracer;
use crate::gpu::frame::main_segment::MainSegment;

/// Where a backdrop break's timestamp sits among its three.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BackdropPhase {
    CaptureStart = 0,
    /// The captures' end and the blurs' start.
    BlurStart = 1,
    BlurEnd = 2,
}

/// A point between passes a frame timestamps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mark {
    /// The start of a run of passes whose span publishes as the pass time, as a bench times one.
    #[cfg(feature = "internals")]
    RunStart,
    #[cfg(feature = "internals")]
    RunEnd,
    /// The backbuffer's copy onto the surface.
    CopyOutStart,
    CopyOutEnd,
    /// The captures and blurs of the frame's backdrop break `at`, at `phase`.
    Backdrop {
        at: usize,
        phase: BackdropPhase,
    },
}

/// The tracer a device takes. The query tracer boxed, as it holds its query sets and staging
/// slots inline; one box per render owner, made with it.
#[derive(Debug)]
enum Tracer {
    Queries(Box<QueryTracer>),
    WallClock(WallClockTracer),
}

/// A render owner's GPU timings: the calls a frame records, the same on every device.
#[derive(Debug)]
pub(crate) struct GpuTimings {
    tracer: Tracer,
}

impl GpuTimings {
    /// A frame's timings for `device`, at `period_ns` per timestamp tick, as finely as `features`
    /// allow.
    pub(crate) fn new(device: &wgpu::Device, period_ns: f32, features: wgpu::Features) -> Self {
        Self::with_tracer(
            device,
            period_ns,
            features,
            features.contains(wgpu::Features::TIMESTAMP_QUERY),
        )
    }

    /// The query tracer when `queries` says the device writes what the timing needs, else the
    /// wall clock.
    fn with_tracer(
        device: &wgpu::Device,
        period_ns: f32,
        features: wgpu::Features,
        queries: bool,
    ) -> Self {
        let tracer = if queries {
            Tracer::Queries(Box::new(QueryTracer::new(device, period_ns, features)))
        } else {
            Tracer::WallClock(WallClockTracer::new())
        };
        Self { tracer }
    }

    /// The `timestamp_writes` of the main pass's `segment`, if this tracer writes them there.
    pub(crate) fn pass_writes(
        &self,
        segment: MainSegment,
    ) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        match &self.tracer {
            Tracer::Queries(t) => t.pass_writes(segment),
            Tracer::WallClock(t) => t.pass_writes(segment),
        }
    }

    /// Open the timing of the main pass's `segment` inside it. Call right after
    /// `begin_render_pass`.
    pub(crate) fn segment_begin(&self, pass: &mut wgpu::RenderPass<'_>, segment: MainSegment) {
        match &self.tracer {
            Tracer::Queries(t) => t.segment_begin(pass, segment),
            Tracer::WallClock(t) => t.segment_begin(pass, segment),
        }
    }

    /// Mark the start of a `kind` of batch inside the main pass.
    pub(crate) fn mark(&self, pass: &mut wgpu::RenderPass<'_>, kind: BatchKind) {
        match &self.tracer {
            Tracer::Queries(t) => t.mark(pass, kind),
            Tracer::WallClock(t) => t.mark(pass, kind),
        }
    }

    /// Close the timing of the main pass's `segment` inside it.
    pub(crate) fn segment_end(&self, pass: &mut wgpu::RenderPass<'_>, segment: MainSegment) {
        match &self.tracer {
            Tracer::Queries(t) => t.segment_end(pass, segment),
            Tracer::WallClock(t) => t.segment_end(pass, segment),
        }
    }

    /// Timestamp `mark` between passes.
    pub(crate) fn encoder_mark(&self, encoder: &mut wgpu::CommandEncoder, mark: Mark) {
        match &self.tracer {
            Tracer::Queries(t) => t.encoder_mark(encoder, mark),
            Tracer::WallClock(t) => t.encoder_mark(encoder, mark),
        }
    }

    /// Close the frame `encoder` recorded, just before `encoder.finish()`.
    pub(crate) fn finish(&mut self, encoder: &mut wgpu::CommandEncoder) {
        match &mut self.tracer {
            Tracer::Queries(t) => t.finish(encoder),
            Tracer::WallClock(t) => t.finish(encoder),
        }
    }

    /// After `queue.submit` on `gpu`: start reading the frame back, and publish into `sink` each
    /// earlier frame that landed.
    pub(crate) fn after_submit(&mut self, gpu: &Gpu, sink: &GpuPassStats) {
        match &mut self.tracer {
            Tracer::Queries(t) => t.after_submit(gpu, sink),
            Tracer::WallClock(t) => t.after_submit(gpu, sink),
        }
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::gpu::frame::gpu_timings::{GpuTimings, Tracer};

    impl GpuTimings {
        /// The timings of a run of passes a bench times between [`Mark::RunStart`] and
        /// [`Mark::RunEnd`]: the query tracer where the device writes timestamps between passes,
        /// else the wall clock.
        ///
        /// [`Mark::RunStart`]: crate::gpu::frame::gpu_timings::Mark::RunStart
        /// [`Mark::RunEnd`]: crate::gpu::frame::gpu_timings::Mark::RunEnd
        pub(crate) fn for_runs(
            device: &wgpu::Device,
            period_ns: f32,
            features: wgpu::Features,
        ) -> Self {
            Self::with_tracer(
                device,
                period_ns,
                features,
                features.contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS),
            )
        }

        /// Record the resolve of every frame the queue said is done into `encoder`, closing no
        /// frame: a run's readback, which no later frame resolves.
        pub(crate) fn resolve_done(&mut self, encoder: &mut wgpu::CommandEncoder) {
            match &mut self.tracer {
                Tracer::Queries(t) => t.resolve_done(encoder),
                Tracer::WallClock(t) => t.resolve_done(encoder),
            }
        }
    }
}
