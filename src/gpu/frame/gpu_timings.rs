//! `wgpu` timestamp-query + pipeline-statistics plumbing.
//!
//! Constructed only when at least `TIMESTAMP_QUERY` is enabled on the
//! device (the host requests it at adapter time when supported — see
//! `DeviceRequirements::GPU_TIMING_FEATURES`). Optionally adds:
//!
//! - **`TIMESTAMP_QUERY_INSIDE_PASSES`** (per-batch timestamps). When
//!   on, we `RenderPass::write_timestamp` at every category transition
//!   inside the main pass (`render_groups`), then attribute the
//!   resolved durations per [`BatchKind`] into the [`GpuPassStats`]
//!   sink. When off, only pass begin/end are timed via descriptor —
//!   `last_pass()` is populated, per-kind slots stay `None`.
//! - **`PIPELINE_STATISTICS_QUERY`**. When on, we bracket the main
//!   pass with `begin_pipeline_statistics_query` /
//!   `end_pipeline_statistics_query` and publish the resolved counts.
//! - **`TIMESTAMP_QUERY_INSIDE_ENCODERS`**. When on, the backbuffer's copy
//!   onto the target after the main pass is bracketed by
//!   `CommandEncoder::write_timestamp` at [`COPY_OUT_INDEX`], and
//!   published as [`GpuPassStats::last_copy_out`].
//!
//! Layout per ping-pong slot (one staging buffer per feature):
//!
//! - `timestamps_buffer`: `QUERY_COUNT * 8` bytes. Layout:
//!     - basic mode (inside-passes off): `[t_begin, t_end]`, count = 2.
//!     - per-batch mode: `[t_begin, t_mid_0, t_mid_1, ..., t_end]`,
//!       count = 2 + n_mid (≤ MAX_TIMESTAMPS).
//!     - then, at [`COPY_OUT_INDEX`], the copy-out's begin and end, on a
//!       frame that copied.
//! - `stats_buffer`: `STATS_FIELD_COUNT * 8` bytes when stats query is
//!   enabled; absent otherwise.
//!
//! Readback is one-frame-lagged (the `map_async` callback fires after
//! the GPU completes the submission). Rigorous benchmarking should use
//! explicit `device.poll(Wait)` and then read the `GpuPassStats`
//! handle (e.g. via `OffscreenHost::gpu_pass_stats`).

use crate::diagnostics::gpu_pass_stats::{BatchKind, GpuPassStats, PipelineStats};
use std::array;
use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering::Acquire, Ordering::Release};

const BYTES_PER_U64: u64 = 8;

/// Max timestamps per frame. Two are always reserved for pass begin /
/// pass end; the remaining slots are filled by per-batch transition
/// writes when `TIMESTAMP_QUERY_INSIDE_PASSES` is on. Sized to comfortably
/// hold a worst-case frame: ~6 distinct categories * a few transition
/// rounds. Excess transitions silently fold into the surrounding
/// category (see [`GpuTimings::mark`]).
const MAX_TIMESTAMPS: u32 = 32;

/// Where the copy-out's begin and end timestamps sit in the query set,
/// past every slot the main pass can use. A query resolve must land on a
/// `QUERY_RESOLVE_BUFFER_ALIGNMENT` offset, and the main pass's slots fill
/// exactly that much, so the copy-out resolves into the same buffer.
const COPY_OUT_INDEX: u32 = MAX_TIMESTAMPS;
const QUERY_COUNT: u32 = COPY_OUT_INDEX + 2;
const COPY_OUT_OFFSET: u64 = COPY_OUT_INDEX as u64 * BYTES_PER_U64;
const TIMESTAMP_BUFFER_BYTES: u64 = QUERY_COUNT as u64 * BYTES_PER_U64;

const _: () = assert!(COPY_OUT_OFFSET.is_multiple_of(wgpu::QUERY_RESOLVE_BUFFER_ALIGNMENT));

/// Number of pipeline-statistics fields we request. Matches the bits
/// set in [`PIPELINE_STATS_FLAGS`]; resolve writes them back in the
/// flag-declaration order (VS_INV, CLIPPER_INV, CLIPPER_OUT, FS_INV,
/// CS_INV).
const STATS_FIELD_COUNT: usize = 5;
const STATS_BUFFER_BYTES: u64 = STATS_FIELD_COUNT as u64 * BYTES_PER_U64;

/// Ping-pong depth — covers GPU one frame behind CPU. If both slots
/// are in-flight (deep stall) we drop the frame's measurement rather
/// than blocking.
const NUM_STAGING: usize = 2;

const TIMESTAMPS_DONE: u8 = 1 << 0;
const STATS_DONE: u8 = 1 << 1;
const TIMESTAMPS_FAILED: u8 = 1 << 2;
const STATS_FAILED: u8 = 1 << 3;

/// All pipeline-statistics fields we care about. Compute is included
/// for layout completeness (always 0 — we have no compute passes).
const PIPELINE_STATS_FLAGS: wgpu::PipelineStatisticsTypes =
    wgpu::PipelineStatisticsTypes::VERTEX_SHADER_INVOCATIONS
        .union(wgpu::PipelineStatisticsTypes::CLIPPER_INVOCATIONS)
        .union(wgpu::PipelineStatisticsTypes::CLIPPER_PRIMITIVES_OUT)
        .union(wgpu::PipelineStatisticsTypes::FRAGMENT_SHADER_INVOCATIONS)
        .union(wgpu::PipelineStatisticsTypes::COMPUTE_SHADER_INVOCATIONS);

#[derive(Debug)]
struct Slot {
    timestamps_buffer: wgpu::Buffer,
    /// Number of valid timestamps written this frame in
    /// `timestamps_buffer[0..count]`. Saved at `resolve()` time so the
    /// async readback path doesn't need to consult the mutable counter.
    timestamps_count: u32,
    /// Kind active during the segment between timestamp `i` and `i+1`,
    /// length `timestamps_count - 1` (zero on basic mode → no per-kind
    /// publish). Saved at `resolve()` time.
    segment_kinds: Vec<BatchKind>,
    /// Whether this frame's copy-out timestamps are in `timestamps_buffer`
    /// at [`COPY_OUT_INDEX`]. Saved at `resolve()` time.
    copied_out: bool,
    stats_buffer: Option<wgpu::Buffer>,
    map_state: Arc<AtomicU8>,
    in_flight: bool,
}

/// Interior-mutable per-frame state. The render-pass walk in
/// `WgpuBackend::render_groups` holds `&self` on the whole backend, so
/// it can't mutate `GpuTimings` directly — these cells let `mark()`
/// bump the timestamp index and append to `segment_kinds` through a
/// shared reference.
#[derive(Debug)]
struct Inner {
    /// Next free index in the timestamp query set. Set to 0 at pass
    /// open. `mark()` writes here on a kind change, then bumps.
    next_index: Cell<u32>,
    /// Kind currently "in flight" between the last timestamp write and
    /// the next. `None` before the first `mark()` (covers the
    /// pass-begin → first-draw setup window, recorded under
    /// [`BatchKind::Setup`]).
    current_kind: Cell<Option<BatchKind>>,
    /// One entry per closed segment — the kind that ran from the
    /// previous timestamp to the one just written. `RefCell` because
    /// `Vec` mutation is rare and uncontended within a frame.
    segment_kinds: RefCell<Vec<BatchKind>>,
    /// Whether [`GpuTimings::copy_out_end`] ran since the last resolve.
    copied_out: Cell<bool>,
}

#[derive(Debug)]
pub(crate) struct GpuTimings {
    /// `MAX_TIMESTAMPS` slots in per-batch mode, 2 in basic mode, then
    /// the copy-out's two at [`COPY_OUT_INDEX`].
    timestamp_query_set: wgpu::QuerySet,
    /// Whether `TIMESTAMP_QUERY_INSIDE_PASSES` is available. False
    /// → only pass begin / end timestamps (via descriptor), no
    /// midpoint writes. Read by [`Self::pass_begin`], [`Self::mark`]
    /// and [`Self::pass_end`], which each no-op when it is false, so
    /// the caller invokes all three unconditionally — the same shape
    /// [`Self::begin_pipeline_stats`] takes for its own feature.
    inside_passes: bool,
    /// Whether `TIMESTAMP_QUERY_INSIDE_ENCODERS` is available. False → the
    /// copy-out is not timed; [`Self::copy_out_begin`] and
    /// [`Self::copy_out_end`] no-op.
    inside_encoders: bool,
    /// `Some` when `PIPELINE_STATISTICS_QUERY` is available.
    stats_query_set: Option<wgpu::QuerySet>,
    /// GPU-visible resolve target for the timestamp query set.
    timestamps_resolve: wgpu::Buffer,
    /// GPU-visible resolve target for the pipeline-statistics query
    /// set (when present).
    stats_resolve: Option<wgpu::Buffer>,
    slots: [Slot; NUM_STAGING],
    pending_slot: Option<usize>,
    /// Cached `queue.get_timestamp_period()` (ticks → ns).
    period_ns: f32,
    inner: Inner,
}

impl GpuTimings {
    pub(crate) fn new(
        device: &wgpu::Device,
        period_ns: f32,
        inside_passes: bool,
        inside_encoders: bool,
        pipeline_stats: bool,
    ) -> Self {
        // Timestamp query set sized for the more permissive mode.
        // Basic mode only uses indices 0 and 1, but the over-allocation
        // is 32 * 8 = 256 bytes, not worth a second code path.
        let timestamp_query_set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("palantir.gpu_timings.timestamps"),
            ty: wgpu::QueryType::Timestamp,
            count: QUERY_COUNT,
        });
        let timestamps_resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("palantir.gpu_timings.timestamps.resolve"),
            size: TIMESTAMP_BUFFER_BYTES,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let (stats_query_set, stats_resolve) = if pipeline_stats {
            let qs = device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("palantir.gpu_timings.stats"),
                ty: wgpu::QueryType::PipelineStatistics(PIPELINE_STATS_FLAGS),
                count: 1,
            });
            let buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("palantir.gpu_timings.stats.resolve"),
                size: STATS_BUFFER_BYTES,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            (Some(qs), Some(buf))
        } else {
            (None, None)
        };

        let slots = array::from_fn(|_| Slot {
            timestamps_buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("palantir.gpu_timings.timestamps.staging"),
                size: TIMESTAMP_BUFFER_BYTES,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            timestamps_count: 0,
            segment_kinds: Vec::new(),
            copied_out: false,
            stats_buffer: pipeline_stats.then(|| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("palantir.gpu_timings.stats.staging"),
                    size: STATS_BUFFER_BYTES,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            }),
            map_state: Arc::new(AtomicU8::new(0)),
            in_flight: false,
        });

        Self {
            timestamp_query_set,
            inside_passes,
            inside_encoders,
            stats_query_set,
            timestamps_resolve,
            stats_resolve,
            slots,
            pending_slot: None,
            period_ns,
            inner: Inner {
                next_index: Cell::new(0),
                current_kind: Cell::new(None),
                segment_kinds: RefCell::new(Vec::with_capacity(MAX_TIMESTAMPS as usize)),
                copied_out: Cell::new(false),
            },
        }
    }

    /// Descriptor for `RenderPassDescriptor::timestamp_writes` in basic
    /// mode. `None` when per-batch mode is active — there we write
    /// pass begin / end inline via `RenderPass::write_timestamp`
    /// instead, so we don't double-write index 0.
    pub(crate) const fn pass_writes(&self) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        if self.inside_passes {
            return None;
        }
        Some(wgpu::RenderPassTimestampWrites {
            query_set: &self.timestamp_query_set,
            beginning_of_pass_write_index: Some(0),
            end_of_pass_write_index: Some(1),
        })
    }

    /// Reset per-frame state, then write the pass-begin timestamp.
    /// No-op outside per-batch mode, where the descriptor's own
    /// begin/end writes already cover the pass. Called immediately
    /// after `begin_render_pass`.
    pub(crate) fn pass_begin(&self, pass: &mut wgpu::RenderPass<'_>) {
        if !self.inside_passes {
            return;
        }
        self.inner.next_index.set(0);
        self.inner.current_kind.set(None);
        self.inner.segment_kinds.borrow_mut().clear();
        pass.write_timestamp(&self.timestamp_query_set, 0);
        self.inner.next_index.set(1);
    }

    /// Mark a category boundary inside the pass. If `kind` matches the
    /// currently-active kind, no-op (no transition). Otherwise writes
    /// one timestamp at the next free index, records the just-closed
    /// segment's kind, and advances. Capacity guard: once the query
    /// set is full minus one (reserved for pass-end), subsequent marks
    /// fold the transition into the current kind silently.
    pub(crate) fn mark(&self, pass: &mut wgpu::RenderPass<'_>, kind: BatchKind) {
        if !self.inside_passes {
            return;
        }
        let cur = self.inner.current_kind.get();
        if cur == Some(kind) {
            return;
        }
        let idx = self.inner.next_index.get();
        // Reserve one slot for the pass-end timestamp.
        if idx >= MAX_TIMESTAMPS - 1 {
            // Fold into current kind — no transition recorded, the
            // overflow simply attributes to the prior kind. Rare in
            // practice (a Partial repaint with >MAX-3 category changes
            // would mean a pathological group stream).
            return;
        }
        pass.write_timestamp(&self.timestamp_query_set, idx);
        let segment_kind = cur.unwrap_or(BatchKind::Setup);
        self.inner.segment_kinds.borrow_mut().push(segment_kind);
        self.inner.current_kind.set(Some(kind));
        self.inner.next_index.set(idx + 1);
    }

    /// Write the pass-end timestamp, closing the final segment. No-op
    /// outside per-batch mode, like [`Self::pass_begin`]. Called
    /// immediately before the pass is dropped.
    pub(crate) fn pass_end(&self, pass: &mut wgpu::RenderPass<'_>) {
        if !self.inside_passes {
            return;
        }
        let idx = self.inner.next_index.get();
        pass.write_timestamp(&self.timestamp_query_set, idx);
        let final_kind = self.inner.current_kind.get().unwrap_or(BatchKind::Setup);
        self.inner.segment_kinds.borrow_mut().push(final_kind);
        self.inner.next_index.set(idx + 1);
    }

    /// Start the pipeline-statistics query around the pass. No-op when
    /// the feature is off.
    pub(crate) fn begin_pipeline_stats(&self, pass: &mut wgpu::RenderPass<'_>) {
        if let Some(qs) = &self.stats_query_set {
            pass.begin_pipeline_statistics_query(qs, 0);
        }
    }

    /// End the pipeline-statistics query. No-op when the feature is off.
    pub(crate) fn end_pipeline_stats(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.stats_query_set.is_some() {
            pass.end_pipeline_statistics_query();
        }
    }

    /// Write the copy-out's begin timestamp, just before the backbuffer is
    /// copied or drawn onto the target. No-op without
    /// `TIMESTAMP_QUERY_INSIDE_ENCODERS`.
    pub(crate) fn copy_out_begin(&self, encoder: &mut wgpu::CommandEncoder) {
        if self.inside_encoders {
            encoder.write_timestamp(&self.timestamp_query_set, COPY_OUT_INDEX);
        }
    }

    /// Write the copy-out's end timestamp, just after it. No-op without
    /// `TIMESTAMP_QUERY_INSIDE_ENCODERS`.
    pub(crate) fn copy_out_end(&self, encoder: &mut wgpu::CommandEncoder) {
        if self.inside_encoders {
            encoder.write_timestamp(&self.timestamp_query_set, COPY_OUT_INDEX + 1);
            self.inner.copied_out.set(true);
        }
    }

    /// Emit `resolve_query_set` + `copy_buffer_to_buffer` into the
    /// caller's encoder. Picks the first idle staging slot; if both
    /// are in-flight, drops this frame's measurement.
    pub(crate) fn resolve(&mut self, encoder: &mut wgpu::CommandEncoder) {
        // Taken before a slot is found, so a dropped measurement does not
        // carry this frame's copy into the next one.
        let copied_out = self.inner.copied_out.replace(false);
        let Some(slot) = (0..NUM_STAGING).find(|&i| !self.slots[i].in_flight) else {
            self.pending_slot = None;
            return;
        };
        self.pending_slot = Some(slot);

        // Timestamps: figure out the actual count. Basic mode = 2 (the
        // descriptor wrote 0/1). Per-batch mode = whatever `mark()` +
        // pass_end accumulated.
        let count = if self.inside_passes {
            self.inner.next_index.get().clamp(2, MAX_TIMESTAMPS)
        } else {
            2
        };
        let bytes = u64::from(count) * BYTES_PER_U64;
        encoder.resolve_query_set(
            &self.timestamp_query_set,
            0..count,
            &self.timestamps_resolve,
            0,
        );
        encoder.copy_buffer_to_buffer(
            &self.timestamps_resolve,
            0,
            &self.slots[slot].timestamps_buffer,
            0,
            bytes,
        );
        self.slots[slot].timestamps_count = count;
        // Copy per-frame segment-kind labels into the slot for the
        // async readback path. Cheap — typically <16 entries.
        let slot_kinds = &mut self.slots[slot].segment_kinds;
        slot_kinds.clear();
        slot_kinds.extend(self.inner.segment_kinds.borrow().iter().copied());

        if copied_out {
            encoder.resolve_query_set(
                &self.timestamp_query_set,
                COPY_OUT_INDEX..QUERY_COUNT,
                &self.timestamps_resolve,
                COPY_OUT_OFFSET,
            );
            encoder.copy_buffer_to_buffer(
                &self.timestamps_resolve,
                COPY_OUT_OFFSET,
                &self.slots[slot].timestamps_buffer,
                COPY_OUT_OFFSET,
                2 * BYTES_PER_U64,
            );
        }
        self.slots[slot].copied_out = copied_out;

        if let (Some(stats_qs), Some(stats_resolve), Some(stats_staging)) = (
            &self.stats_query_set,
            &self.stats_resolve,
            &self.slots[slot].stats_buffer,
        ) {
            encoder.resolve_query_set(stats_qs, 0..1, stats_resolve, 0);
            encoder.copy_buffer_to_buffer(stats_resolve, 0, stats_staging, 0, STATS_BUFFER_BYTES);
        }

        self.slots[slot].in_flight = true;
    }

    /// Call after `queue.submit`. Kicks an async map on the just-
    /// written slot, polls the device so prior `map_async` callbacks
    /// fire, and publishes any slot whose readback has landed into
    /// [`GpuPassStats`].
    pub(crate) fn after_submit(&mut self, device: &wgpu::Device, sink: &GpuPassStats) {
        if let Some(slot_idx) = self.pending_slot.take() {
            let map_state = Arc::clone(&self.slots[slot_idx].map_state);
            map_state.store(0, Release);
            self.slots[slot_idx].timestamps_buffer.slice(..).map_async(
                wgpu::MapMode::Read,
                move |res| {
                    let failed = if res.is_err() { TIMESTAMPS_FAILED } else { 0 };
                    map_state.fetch_or(TIMESTAMPS_DONE | failed, Release);
                },
            );
            if let Some(stats_buf) = &self.slots[slot_idx].stats_buffer {
                let map_state = Arc::clone(&self.slots[slot_idx].map_state);
                stats_buf
                    .slice(..)
                    .map_async(wgpu::MapMode::Read, move |res| {
                        let failed = if res.is_err() { STATS_FAILED } else { 0 };
                        map_state.fetch_or(STATS_DONE | failed, Release);
                    });
            }
        }
        let _ = device.poll(wgpu::PollType::Poll);
        for slot in &mut self.slots {
            if !slot.in_flight {
                continue;
            }
            let state = slot.map_state.load(Acquire);
            if !mappings_complete(state, slot.stats_buffer.is_some()) {
                continue;
            }
            if mappings_failed(state) {
                discard_slot(slot, state);
            } else {
                consume_slot(slot, self.period_ns, sink);
            }
            slot.map_state.store(0, Release);
            slot.in_flight = false;
        }
    }
}

const fn mappings_complete(state: u8, has_stats: bool) -> bool {
    let required = TIMESTAMPS_DONE | if has_stats { STATS_DONE } else { 0 };
    state & required == required
}

const fn mappings_failed(state: u8) -> bool {
    state & (TIMESTAMPS_FAILED | STATS_FAILED) != 0
}

fn discard_slot(slot: &Slot, state: u8) {
    if state & TIMESTAMPS_FAILED == 0 {
        slot.timestamps_buffer.unmap();
    }
    if state & STATS_FAILED == 0
        && let Some(stats_buf) = &slot.stats_buffer
    {
        stats_buf.unmap();
    }
}

/// Read the mapped buffers on `slot`, publish into `sink`, then unmap.
/// Caller is responsible for clearing `in_flight` after this returns.
fn consume_slot(slot: &mut Slot, period_ns: f32, sink: &GpuPassStats) {
    let ts_range = slot
        .timestamps_buffer
        .slice(..)
        .get_mapped_range()
        .expect("map timestamps range");
    publish_timestamps(
        &ts_range,
        slot.timestamps_count as usize,
        &slot.segment_kinds,
        period_ns,
        sink,
    );
    publish_copy_out(&ts_range, slot.copied_out, period_ns, sink);
    drop(ts_range);
    slot.timestamps_buffer.unmap();

    if let Some(stats_buf) = &slot.stats_buffer {
        let s_range = stats_buf
            .slice(..)
            .get_mapped_range()
            .expect("map stats range");
        publish_stats(&s_range, sink);
        drop(s_range);
        stats_buf.unmap();
    }
}

/// Parse `count` resolved timestamps and publish pass + per-kind
/// durations into `sink`. Split from [`consume_slot`] so the publish
/// rules are testable without wgpu buffers.
fn publish_timestamps(
    ts: &[u8],
    count: usize,
    segment_kinds: &[BatchKind],
    period_ns: f32,
    sink: &GpuPassStats,
) {
    // Always: pass duration = last - first. Clear the per-kind table
    // on *every* measured frame, not only ones with midpoint marks —
    // a begin/end-only frame (blank window in per-batch mode) must
    // not leave the previous frame's per-kind values published.
    if count >= 2 {
        sink.record_pass_ns(span_ns(ts, 0, count - 1, period_ns));
        sink.clear_kinds();
    }
    // Per-batch attribution when we collected midpoint marks.
    if count >= 3 {
        let mut per_kind_ns = [0u64; BatchKind::COUNT];
        let mut seen = [false; BatchKind::COUNT];
        for i in 0..count - 1 {
            let kind = segment_kinds.get(i).copied().unwrap_or(BatchKind::Setup);
            let seg_ns = span_ns(ts, i, i + 1, period_ns);
            seen[kind.idx()] = true;
            per_kind_ns[kind.idx()] = per_kind_ns[kind.idx()].saturating_add(seg_ns);
        }
        for kind in BatchKind::ALL {
            if seen[kind.idx()] {
                sink.record_kind_ns(kind, per_kind_ns[kind.idx()]);
            }
        }
    }
}

/// Publish the copy-out's duration from its two timestamps at
/// [`COPY_OUT_INDEX`], or clear it when the frame copied nothing.
fn publish_copy_out(ts: &[u8], copied_out: bool, period_ns: f32, sink: &GpuPassStats) {
    let begin = COPY_OUT_INDEX as usize;
    let ns = copied_out.then(|| span_ns(ts, begin, begin + 1, period_ns));
    sink.record_copy_out_ns(ns);
}

/// Nanoseconds from timestamp `from` to timestamp `to` of a resolved
/// query buffer, at `period_ns` per tick. A later write that resolved to
/// an earlier tick reads as zero.
#[expect(
    clippy::cast_sign_loss,
    reason = "a tick delta is a saturating difference and the period is positive, so the product is never negative"
)]
fn span_ns(ts: &[u8], from: usize, to: usize, period_ns: f32) -> u64 {
    (tick(ts, to).saturating_sub(tick(ts, from)) as f64 * f64::from(period_ns)) as u64
}

/// The `index`th 64-bit word of a resolved query buffer.
///
/// The one decode. `pod_read_unaligned` rather than
/// `bytemuck::cast_slice`, which the backend reaches for elsewhere: a
/// mapped range's alignment is the driver's to promise, and a cast that
/// panicked on a misaligned mapping would take the frame down over a
/// debug counter. The read compiles to the same load either way.
#[inline]
fn tick(bytes: &[u8], index: usize) -> u64 {
    let off = index * 8;
    bytemuck::pod_read_unaligned(&bytes[off..off + 8])
}

/// Parse the resolved pipeline-statistics counters and publish them.
/// Field order matches [`PIPELINE_STATS_FLAGS`] — the mapping lives
/// here, next to the flag declaration that defines it.
fn publish_stats(bytes: &[u8], sink: &GpuPassStats) {
    let mut values = [0u64; STATS_FIELD_COUNT];
    for (i, v) in values.iter_mut().enumerate() {
        *v = tick(bytes, i);
    }
    sink.record_pipeline_stats(PipelineStats {
        vertex_shader_invocations: values[0],
        clipper_invocations: values[1],
        clipper_primitives_out: values[2],
        fragment_shader_invocations: values[3],
        compute_shader_invocations: values[4],
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn ts_bytes(ticks: &[u64]) -> Vec<u8> {
        ticks.iter().flat_map(|t| t.to_le_bytes()).collect()
    }

    #[test]
    fn per_kind_publish_distinguishes_absent_zero_and_blank() {
        let sink = GpuPassStats::default();

        // Frame 1: timestamps [1000, 1001, 3001, 5001] at 0.5 ns/tick:
        //   pass    = floor((5001 - 1000) * 0.5) = 2000 ns
        //   quads   = floor((1001 - 1000) * 0.5) = 0 ns
        //   shadows = floor((3001 - 1001) * 0.5) = 1000 ns
        //   text    = floor((5001 - 3001) * 0.5) = 1000 ns
        publish_timestamps(
            &ts_bytes(&[1000, 1001, 3001, 5001]),
            4,
            &[BatchKind::Quads, BatchKind::Shadows, BatchKind::Text],
            0.5,
            &sink,
        );
        assert_eq!(sink.last_pass(), Some(Duration::from_micros(2)));
        assert_eq!(sink.last_kind(BatchKind::Quads), Some(Duration::ZERO));
        assert_eq!(
            sink.last_kind(BatchKind::Shadows),
            Some(Duration::from_micros(1))
        );
        assert_eq!(
            sink.last_kind(BatchKind::Text),
            Some(Duration::from_micros(1))
        );
        assert_eq!(sink.last_kind(BatchKind::Mesh), None);

        // Frame 2: begin/end only (count == 2 — a truly blank window
        // in per-batch mode). Pass time refreshes to 14000 - 10000 =
        // 4000 ns; every per-kind slot clears to None instead of
        // keeping frame 1's values.
        publish_timestamps(&ts_bytes(&[10_000, 14_000]), 2, &[], 1.0, &sink);
        assert_eq!(sink.last_pass(), Some(Duration::from_micros(4)));
        for kind in BatchKind::ALL {
            assert_eq!(
                sink.last_kind(kind),
                None,
                "{} stale after blank measured frame",
                kind.label(),
            );
        }
    }

    /// A frame that copied publishes its copy-out from the two timestamps
    /// at `COPY_OUT_INDEX`; the next frame that copied nothing clears it,
    /// so an earlier copy does not keep showing. Ticks 7 000 → 7 600 at
    /// 0.5 ns/tick: `floor(600 · 0.5)` = 300 ns.
    #[test]
    fn copy_out_publishes_its_two_timestamps_or_clears() {
        let sink = GpuPassStats::default();
        let mut ticks = vec![0; QUERY_COUNT as usize];
        ticks[COPY_OUT_INDEX as usize] = 7_000;
        ticks[COPY_OUT_INDEX as usize + 1] = 7_600;
        let bytes = ts_bytes(&ticks);
        publish_copy_out(&bytes, true, 0.5, &sink);
        assert_eq!(sink.last_copy_out(), Some(Duration::from_nanos(300)));
        publish_copy_out(&bytes, false, 0.5, &sink);
        assert_eq!(sink.last_copy_out(), None, "a frame with no copy clears it");
    }

    #[test]
    fn stats_publish_in_flag_declaration_order() {
        let sink = GpuPassStats::default();
        publish_stats(&ts_bytes(&[10, 20, 30, 40, 0]), &sink);
        let s = sink.last_pipeline_stats().expect("published");
        assert_eq!(s.vertex_shader_invocations, 10);
        assert_eq!(s.clipper_invocations, 20);
        assert_eq!(s.clipper_primitives_out, 30);
        assert_eq!(s.fragment_shader_invocations, 40);
        assert_eq!(s.compute_shader_invocations, 0);
    }

    #[test]
    fn readback_waits_for_every_mapping_and_reports_failures() {
        assert!(!mappings_complete(0, false));
        assert!(mappings_complete(TIMESTAMPS_DONE, false));

        assert!(!mappings_complete(TIMESTAMPS_DONE, true));
        assert!(!mappings_complete(STATS_DONE, true));
        assert!(mappings_complete(TIMESTAMPS_DONE | STATS_DONE, true));

        assert!(!mappings_failed(TIMESTAMPS_DONE | STATS_DONE));
        assert!(mappings_failed(TIMESTAMPS_DONE | TIMESTAMPS_FAILED));
        assert!(mappings_failed(STATS_DONE | STATS_FAILED));
    }
}
