//! The tracer of a device that writes no timestamps where a timing needs them: each frame timed on
//! the CPU clock, from its submit to the queue telling the CPU its work is done.

use crate::diagnostics::gpu_pass_stats::{BackdropTimes, BatchKind, GpuPassStats};
use crate::gpu::device::gpu::Gpu;
use crate::gpu::frame::gpu_timings::Mark;
use crate::gpu::frame::main_segment::MainSegment;
use std::array;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering::Acquire, Ordering::Release};
use std::time::{Duration, Instant};

/// Frames timed at once; a frame submitted while every slot waits goes unmeasured, as a query
/// tracer drops one while every slot is busy.
const SLOTS: usize = 2;

/// Not done yet: a done time is a nanosecond count past the tracer's origin, so never this.
const PENDING: u64 = u64::MAX;

/// One timed frame: when it was submitted, and when the queue said it was done.
#[derive(Debug)]
struct Slot {
    submitted: u64,
    /// Written by the queue's callback; [`PENDING`] until then.
    done: Arc<AtomicU64>,
    in_flight: bool,
}

/// Times each frame from its submit to the queue's work-done callback, on the CPU clock, so the
/// span also holds the submit's and the wake-up's latency: no marks inside or between passes,
/// and no pipeline statistics.
#[derive(Debug)]
pub(super) struct WallClockTracer {
    origin: Instant,
    slots: [Slot; SLOTS],
    /// The slot [`Self::finish`] took for the frame it closed, which `after_submit` starts timing.
    pending: Option<usize>,
}

impl WallClockTracer {
    pub(super) fn new() -> Self {
        Self {
            origin: Instant::now(),
            slots: array::from_fn(|_| Slot {
                submitted: 0,
                done: Arc::new(AtomicU64::new(PENDING)),
                in_flight: false,
            }),
            pending: None,
        }
    }

    pub(super) const fn pass_writes(
        &self,
        _segment: MainSegment,
    ) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        None
    }

    pub(super) const fn segment_begin(
        &self,
        _pass: &mut wgpu::RenderPass<'_>,
        _segment: MainSegment,
    ) {
    }

    pub(super) const fn mark(&self, _pass: &mut wgpu::RenderPass<'_>, _kind: BatchKind) {}

    pub(super) const fn segment_end(
        &self,
        _pass: &mut wgpu::RenderPass<'_>,
        _segment: MainSegment,
    ) {
    }

    pub(super) const fn encoder_mark(&self, _encoder: &mut wgpu::CommandEncoder, _mark: Mark) {}

    /// Nothing to resolve: the CPU clock read the frames.
    #[cfg(feature = "internals")]
    pub(super) const fn resolve_done(&self, _encoder: &mut wgpu::CommandEncoder) {}

    /// Take an idle slot for the frame `encoder` closes; with every slot waiting, the frame goes
    /// unmeasured.
    pub(super) fn finish(&mut self, _encoder: &mut wgpu::CommandEncoder) {
        self.pending = self.slots.iter().position(|slot| !slot.in_flight);
    }

    /// Start timing the frame [`Self::finish`] closed, just submitted, and publish into `sink`
    /// each timed frame the queue said is done: its span as the pass time, every finer time
    /// absent. A call with no frame closed since only publishes.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "nanoseconds since the tracer opened fit a u64 for 584 years"
    )]
    pub(super) fn after_submit(&mut self, gpu: &Gpu, sink: &GpuPassStats) {
        let origin = self.origin;
        if let Some(slot) = self.pending.take().map(|at| &mut self.slots[at]) {
            slot.submitted = origin.elapsed().as_nanos() as u64;
            slot.done.store(PENDING, Release);
            slot.in_flight = true;
            let done = Arc::clone(&slot.done);
            gpu.queue.on_submitted_work_done(move || {
                done.store(origin.elapsed().as_nanos() as u64, Release);
            });
        }
        for slot in self.slots.iter_mut().filter(|slot| slot.in_flight) {
            let done = slot.done.load(Acquire);
            if done == PENDING {
                continue;
            }
            slot.in_flight = false;
            let elapsed = Duration::from_nanos(done.saturating_sub(slot.submitted));
            sink.record_pass_ns(u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX));
            sink.clear_kinds();
            sink.record_copy_out_ns(None);
            sink.record_backdrop_ns(BackdropTimes::default());
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::diagnostics::gpu_pass_stats::{BackdropTimes, BatchKind, GpuPassStats};
    use crate::gpu::frame::gpu_timings::wall_clock_tracer::WallClockTracer;
    use crate::gpu::test_gpu::headless_test_gpu;

    /// A frame `finish` closed is timed from its submit to the queue's callback and publishes as
    /// the pass time, every finer time absent; a call with no frame closed since starts nothing,
    /// so a second collect after the wait publishes that frame and no other.
    #[test]
    fn a_closed_frame_publishes_its_wall_time_once() {
        let lease = headless_test_gpu();
        let gpu = lease.handles();
        let mut tracer = WallClockTracer::new();
        let sink = GpuPassStats::default();
        sink.record_kind_ns(BatchKind::Quads, 5);
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        tracer.finish(&mut encoder);
        gpu.queue.submit([encoder.finish()]);
        tracer.after_submit(&gpu, &sink);
        assert_eq!(
            tracer.slots.iter().filter(|slot| slot.in_flight).count()
                + usize::from(sink.last_pass().is_some()),
            1,
            "one frame timed"
        );
        lease.wait();
        tracer.after_submit(&gpu, &sink);
        assert!(sink.last_pass().is_some(), "the frame published");
        assert!(
            tracer.slots.iter().all(|slot| !slot.in_flight),
            "no frame started without a finish"
        );
        assert_eq!(sink.last_kind(BatchKind::Quads), None);
        assert_eq!(sink.last_copy_out(), None);
        assert_eq!(sink.last_backdrops(), BackdropTimes::default());
    }
}
