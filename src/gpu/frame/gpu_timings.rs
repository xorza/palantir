//! `wgpu` timestamp-query and pipeline-statistics plumbing, constructed only when `TIMESTAMP_QUERY` is enabled (see `DeviceRequirements::GPU_TIMING_FEATURES`).
//!
//! - `TIMESTAMP_QUERY_INSIDE_PASSES` writes a timestamp at every category transition in `render_groups`, attributed per [`BatchKind`]; off, only pass begin/end are timed.
//! - `PIPELINE_STATISTICS_QUERY` brackets the main pass.
//! - `TIMESTAMP_QUERY_INSIDE_ENCODERS` brackets the backbuffer copy-out at [`COPY_OUT_INDEX`], published as [`GpuPassStats::last_copy_out`].
//!
//! Readback is one frame lagged (`map_async`); benchmarks should `device.poll(Wait)` first.

use crate::diagnostics::gpu_pass_stats::{BatchKind, GpuPassStats, PipelineStats};
use std::array;
use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering::Acquire, Ordering::Release};

const BYTES_PER_U64: u64 = 8;

/// Max timestamps per frame: two reserved for pass begin/end, the rest for per-batch transitions; excess transitions fold into the surrounding category.
const MAX_TIMESTAMPS: u32 = 32;

/// Where the copy-out's timestamps sit, past every main-pass slot; a query resolve must land on a `QUERY_RESOLVE_BUFFER_ALIGNMENT` offset, which the main pass's slots fill exactly.
const COPY_OUT_INDEX: u32 = MAX_TIMESTAMPS;
const QUERY_COUNT: u32 = COPY_OUT_INDEX + 2;
const COPY_OUT_OFFSET: u64 = COPY_OUT_INDEX as u64 * BYTES_PER_U64;
const TIMESTAMP_BUFFER_BYTES: u64 = QUERY_COUNT as u64 * BYTES_PER_U64;

const _: () = assert!(COPY_OUT_OFFSET.is_multiple_of(wgpu::QUERY_RESOLVE_BUFFER_ALIGNMENT));

const STATS_FIELD_COUNT: usize = 5;
const STATS_BUFFER_BYTES: u64 = STATS_FIELD_COUNT as u64 * BYTES_PER_U64;

/// Ping-pong depth for a GPU one frame behind; with both slots in flight the measurement is dropped.
const NUM_STAGING: usize = 2;

const TIMESTAMPS_DONE: u8 = 1 << 0;
const STATS_DONE: u8 = 1 << 1;
const TIMESTAMPS_FAILED: u8 = 1 << 2;
const STATS_FAILED: u8 = 1 << 3;

const PIPELINE_STATS_FLAGS: wgpu::PipelineStatisticsTypes =
    wgpu::PipelineStatisticsTypes::VERTEX_SHADER_INVOCATIONS
        .union(wgpu::PipelineStatisticsTypes::CLIPPER_INVOCATIONS)
        .union(wgpu::PipelineStatisticsTypes::CLIPPER_PRIMITIVES_OUT)
        .union(wgpu::PipelineStatisticsTypes::FRAGMENT_SHADER_INVOCATIONS)
        .union(wgpu::PipelineStatisticsTypes::COMPUTE_SHADER_INVOCATIONS);

#[derive(Debug)]
struct Slot {
    timestamps_buffer: wgpu::Buffer,
    timestamps_count: u32,
    /// Kind of the segment between timestamps `i` and `i+1`, saved at `resolve()`; empty in basic mode.
    segment_kinds: Vec<BatchKind>,
    copied_out: bool,
    stats_buffer: Option<wgpu::Buffer>,
    map_state: Arc<AtomicU8>,
    in_flight: bool,
}

/// Interior-mutable per-frame state: `render_groups` holds `&self`, so `mark()` mutates through these.
#[derive(Debug)]
struct Inner {
    next_index: Cell<u32>,
    /// Kind in flight since the last timestamp; `None` before the first `mark()` (recorded as [`BatchKind::Setup`]).
    current_kind: Cell<Option<BatchKind>>,
    segment_kinds: RefCell<Vec<BatchKind>>,
    copied_out: Cell<bool>,
}

#[derive(Debug)]
pub(crate) struct GpuTimings {
    timestamp_query_set: wgpu::QuerySet,
    /// Whether `TIMESTAMP_QUERY_INSIDE_PASSES` is available; off, [`Self::pass_begin`], [`Self::mark`] and [`Self::pass_end`] no-op.
    inside_passes: bool,
    /// Whether `TIMESTAMP_QUERY_INSIDE_ENCODERS` is available; off, the copy-out is not timed.
    inside_encoders: bool,
    stats_query_set: Option<wgpu::QuerySet>,
    timestamps_resolve: wgpu::Buffer,
    stats_resolve: Option<wgpu::Buffer>,
    slots: [Slot; NUM_STAGING],
    pending_slot: Option<usize>,
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

    /// Descriptor for `timestamp_writes` in basic mode; `None` in per-batch mode, which writes begin/end inline.
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

    /// Resets per-frame state and writes the pass-begin timestamp; per-batch mode only. Call right after `begin_render_pass`.
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

    /// Marks a category boundary: no-op if `kind` is active, else writes a timestamp and records the closed segment. Once the set is full minus the pass-end slot, transitions fold into the current kind.
    pub(crate) fn mark(&self, pass: &mut wgpu::RenderPass<'_>, kind: BatchKind) {
        if !self.inside_passes {
            return;
        }
        let cur = self.inner.current_kind.get();
        if cur == Some(kind) {
            return;
        }
        let idx = self.inner.next_index.get();
        if idx >= MAX_TIMESTAMPS - 1 {
            return;
        }
        pass.write_timestamp(&self.timestamp_query_set, idx);
        let segment_kind = cur.unwrap_or(BatchKind::Setup);
        self.inner.segment_kinds.borrow_mut().push(segment_kind);
        self.inner.current_kind.set(Some(kind));
        self.inner.next_index.set(idx + 1);
    }

    /// Writes the pass-end timestamp, closing the final segment; per-batch mode only.
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

    pub(crate) fn begin_pipeline_stats(&self, pass: &mut wgpu::RenderPass<'_>) {
        if let Some(qs) = &self.stats_query_set {
            pass.begin_pipeline_statistics_query(qs, 0);
        }
    }

    pub(crate) fn end_pipeline_stats(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.stats_query_set.is_some() {
            pass.end_pipeline_statistics_query();
        }
    }

    pub(crate) fn copy_out_begin(&self, encoder: &mut wgpu::CommandEncoder) {
        if self.inside_encoders {
            encoder.write_timestamp(&self.timestamp_query_set, COPY_OUT_INDEX);
        }
    }

    pub(crate) fn copy_out_end(&self, encoder: &mut wgpu::CommandEncoder) {
        if self.inside_encoders {
            encoder.write_timestamp(&self.timestamp_query_set, COPY_OUT_INDEX + 1);
            self.inner.copied_out.set(true);
        }
    }

    /// Emits `resolve_query_set` and `copy_buffer_to_buffer` using the first idle staging slot; with both in flight the measurement is dropped.
    pub(crate) fn resolve(&mut self, encoder: &mut wgpu::CommandEncoder) {
        // Taken before a slot is found, so a dropped measurement doesn't carry its copy into the next frame.
        let copied_out = self.inner.copied_out.replace(false);
        let Some(slot) = (0..NUM_STAGING).find(|&i| !self.slots[i].in_flight) else {
            self.pending_slot = None;
            return;
        };
        self.pending_slot = Some(slot);

        // Basic mode is 2 (the descriptor wrote 0/1); per-batch is what `mark()` and `pass_end` accumulated.
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

    /// Call after `queue.submit`: kicks an async map, polls so prior callbacks fire, and publishes landed readbacks into [`GpuPassStats`].
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

/// Parses `count` resolved timestamps and publishes pass and per-kind durations; split from [`consume_slot`] to test without wgpu buffers.
fn publish_timestamps(
    ts: &[u8],
    count: usize,
    segment_kinds: &[BatchKind],
    period_ns: f32,
    sink: &GpuPassStats,
) {
    // Pass duration = last - first. The per-kind table clears on every measured frame, so a begin/end-only frame doesn't keep stale values.
    if count >= 2 {
        sink.record_pass_ns(span_ns(ts, 0, count - 1, period_ns));
        sink.clear_kinds();
    }
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

fn publish_copy_out(ts: &[u8], copied_out: bool, period_ns: f32, sink: &GpuPassStats) {
    let begin = COPY_OUT_INDEX as usize;
    let ns = copied_out.then(|| span_ns(ts, begin, begin + 1, period_ns));
    sink.record_copy_out_ns(ns);
}

#[expect(
    clippy::cast_sign_loss,
    reason = "a tick delta is a saturating difference and the period is positive, so the product is never negative"
)]
fn span_ns(ts: &[u8], from: usize, to: usize, period_ns: f32) -> u64 {
    (tick(ts, to).saturating_sub(tick(ts, from)) as f64 * f64::from(period_ns)) as u64
}

/// The `index`th 64-bit word of a resolved query buffer. `pod_read_unaligned` rather than `bytemuck::cast_slice`: a mapped range's alignment is the driver's to promise, and a panicking cast would take the frame down over a debug counter.
#[inline]
fn tick(bytes: &[u8], index: usize) -> u64 {
    let off = index * 8;
    bytemuck::pod_read_unaligned(&bytes[off..off + 8])
}

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

        // Frame 1: timestamps [1000, 1001, 3001, 5001] at 0.5 ns/tick: pass floor(4001 * 0.5) = 2000, quads floor(1 * 0.5) = 0, shadows 1000, text 1000 ns.
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

        // Frame 2: begin/end only (a blank window): pass 14000 - 10000 = 4000 ns; per-kind slots clear to None.
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

    /// A frame that copied publishes its copy-out from the two timestamps at `COPY_OUT_INDEX`; the next that copied nothing clears it. Ticks 7000 to 7600 at 0.5 ns/tick: `floor(600 * 0.5)` = 300 ns.
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
