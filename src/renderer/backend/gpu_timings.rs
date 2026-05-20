//! `wgpu` timestamp-query plumbing for the main render pass.
//!
//! Constructed only when the device has [`wgpu::Features::TIMESTAMP_QUERY`]
//! enabled (the host requests it at adapter time when supported —
//! see `winit_host.rs`). Owns one `QuerySet` (2 timestamps: pass
//! begin + end), one resolve buffer (GPU-visible target of
//! `resolve_query_set`), and a ping-pong pair of mappable staging
//! buffers so we can `map_async` last frame's data while this frame
//! writes a fresh pair without contention.
//!
//! Lifecycle per frame:
//!
//! 1. [`Self::pass_writes`] returns a `RenderPassTimestampWrites`
//!    descriptor; the caller attaches it to the main pass.
//! 2. After the pass closes (but before `queue.submit`), the caller
//!    invokes [`Self::resolve`] which emits `resolve_query_set` +
//!    `copy_buffer_to_buffer` into the encoder, targeting the next
//!    idle staging slot.
//! 3. After `queue.submit` returns, [`Self::after_submit`] kicks an
//!    async map on the just-written slot and reads back any slots
//!    whose map has completed since last frame, publishing the
//!    delta into [`super::gpu_pass_stats`].
//!
//! Readback is one-frame-lagged (the map_async callback fires after
//! the GPU completes the submission). The debug overlay surfaces the
//! latest value, which is "what the GPU was doing one frame ago" —
//! close enough for human eyes; rigorous benchmarking should use
//! `frame_stats::take()` snapshots after explicit `device.poll(Wait)`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

/// Begin + end of the main pass.
const N_TIMESTAMPS: u32 = 2;
const BYTES_PER_TIMESTAMP: u64 = 8;
const RESOLVE_BYTES: u64 = N_TIMESTAMPS as u64 * BYTES_PER_TIMESTAMP;

/// Ping-pong depth. 2 covers the typical "GPU one frame behind CPU"
/// pipeline depth — if both slots are in-flight on a given frame
/// (deep stall), we drop that frame's measurement rather than
/// blocking on the map. Bumping past 2 didn't change the observed
/// hit rate in practice.
const NUM_STAGING: usize = 2;

/// Per-staging slot state. The `ready` flag is set by the `map_async`
/// callback (which runs off the device-polling thread, hence
/// `Arc<AtomicBool>` so the closure can outlive the borrow); `in_flight`
/// is set when we enqueue the copy and cleared after we read the
/// mapped bytes back out.
struct Slot {
    buffer: wgpu::Buffer,
    ready: Arc<AtomicBool>,
    in_flight: bool,
}

pub(super) struct GpuTimings {
    query_set: wgpu::QuerySet,
    resolve_buffer: wgpu::Buffer,
    slots: [Slot; NUM_STAGING],
    /// Index of the slot written this frame (set by [`Self::resolve`]
    /// so [`Self::after_submit`] knows which slot to map).
    write_index: usize,
    /// `queue.get_timestamp_period()` — ticks → nanoseconds factor.
    /// Cached at construction; not expected to change at runtime.
    period_ns: f32,
}

impl GpuTimings {
    pub(super) fn new(device: &wgpu::Device, period_ns: f32) -> Self {
        let query_set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("palantir.gpu_timings.queries"),
            ty: wgpu::QueryType::Timestamp,
            count: N_TIMESTAMPS,
        });
        let resolve_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("palantir.gpu_timings.resolve"),
            size: RESOLVE_BYTES,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let slots = std::array::from_fn(|_| Slot {
            buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("palantir.gpu_timings.staging"),
                size: RESOLVE_BYTES,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            ready: Arc::new(AtomicBool::new(false)),
            in_flight: false,
        });
        Self {
            query_set,
            resolve_buffer,
            slots,
            write_index: 0,
            period_ns,
        }
    }

    /// Descriptor to attach to the main pass's
    /// `RenderPassDescriptor::timestamp_writes`. Writes index 0 at
    /// pass begin and index 1 at pass end.
    pub(super) fn pass_writes(&self) -> wgpu::RenderPassTimestampWrites<'_> {
        wgpu::RenderPassTimestampWrites {
            query_set: &self.query_set,
            beginning_of_pass_write_index: Some(0),
            end_of_pass_write_index: Some(1),
        }
    }

    /// Emit `resolve_query_set` + `copy_buffer_to_buffer` into the
    /// caller's encoder. Picks the first idle staging slot; if both
    /// are in-flight (caller's GPU is more than one frame behind),
    /// silently drops this frame's measurement.
    pub(super) fn resolve(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let Some(slot) = (0..NUM_STAGING).find(|&i| !self.slots[i].in_flight) else {
            return;
        };
        self.write_index = slot;
        encoder.resolve_query_set(&self.query_set, 0..N_TIMESTAMPS, &self.resolve_buffer, 0);
        encoder.copy_buffer_to_buffer(
            &self.resolve_buffer,
            0,
            &self.slots[slot].buffer,
            0,
            RESOLVE_BYTES,
        );
        self.slots[slot].in_flight = true;
    }

    /// Call after `queue.submit`. Kicks an async map on the slot
    /// just written, polls the device so any prior map_async
    /// callbacks fire, and publishes any slot whose readback has
    /// landed into [`super::gpu_pass_stats`].
    pub(super) fn after_submit(&mut self, device: &wgpu::Device) {
        let slot_idx = self.write_index;
        if self.slots[slot_idx].in_flight {
            let ready = self.slots[slot_idx].ready.clone();
            self.slots[slot_idx]
                .buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |res| {
                    if res.is_ok() {
                        ready.store(true, Relaxed);
                    }
                });
        }
        // Non-blocking poll so any in-flight map callbacks fire.
        let _ = device.poll(wgpu::PollType::Poll);
        for slot in &mut self.slots {
            if slot.in_flight && slot.ready.load(Relaxed) {
                let range = slot.buffer.slice(..).get_mapped_range();
                let t0 = u64::from_le_bytes(range[..8].try_into().unwrap());
                let t1 = u64::from_le_bytes(range[8..16].try_into().unwrap());
                drop(range);
                slot.buffer.unmap();
                let delta_ns = (t1.saturating_sub(t0) as f64 * self.period_ns as f64) as u64;
                super::gpu_pass_stats::record_pass_ns(delta_ns);
                slot.ready.store(false, Relaxed);
                slot.in_flight = false;
            }
        }
    }
}
