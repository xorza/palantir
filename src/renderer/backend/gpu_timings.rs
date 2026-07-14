//! `wgpu` timestamp-query + pipeline-statistics plumbing.
//!
//! Constructed only when at least `TIMESTAMP_QUERY` is enabled on the
//! device (the host requests it at adapter time when supported — see
//! `winit_host.rs`). Optionally adds:
//!
//! - **`TIMESTAMP_QUERY_INSIDE_PASSES`** (per-batch timestamps). When
//!   on, we `RenderPass::write_timestamp` at every category transition
//!   inside the main pass (`render_groups`), then attribute the
//!   resolved durations per [`BatchKind`] into the [`GpuPassStats`]
//!   sink. When off, only pass begin/end are timed via descriptor —
//!   `last_pass_ms()` is populated, per-kind slots stay `None`.
//! - **`PIPELINE_STATISTICS_QUERY`**. When on, we bracket the main
//!   pass with `begin_pipeline_statistics_query` /
//!   `end_pipeline_statistics_query` and publish the resolved counts.
//!
//! Layout per ping-pong slot (one staging buffer per feature):
//!
//! - `timestamps_buffer`: `MAX_TIMESTAMPS * 8` bytes. Layout:
//!     - basic mode (inside-passes off): `[t_begin, t_end]`, count = 2.
//!     - per-batch mode: `[t_begin, t_mid_0, t_mid_1, ..., t_end]`,
//!       count = 2 + n_mid (≤ MAX_TIMESTAMPS).
//! - `stats_buffer`: `STATS_FIELD_COUNT * 8` bytes when stats query is
//!   enabled; absent otherwise.
//!
//! Readback is one-frame-lagged (the `map_async` callback fires after
//! the GPU completes the submission). Rigorous benchmarking should use
//! explicit `device.poll(Wait)` and then read the `GpuPassStats`
//! handle (e.g. via `WindowRenderer::gpu_pass_stats`).

use crate::renderer::backend::gpu_pass_stats::{BatchKind, GpuPassStats, PipelineStats};
use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering::Acquire, Ordering::Release};
use strum::IntoEnumIterator;

const BYTES_PER_U64: u64 = 8;

/// Max timestamps per frame. Two are always reserved for pass begin /
/// pass end; the remaining slots are filled by per-batch transition
/// writes when `TIMESTAMP_QUERY_INSIDE_PASSES` is on. Sized to comfortably
/// hold a worst-case frame: ~6 distinct categories * a few transition
/// rounds. Excess transitions silently fold into the surrounding
/// category (see [`Inner::mark`]).
const MAX_TIMESTAMPS: u32 = 32;
const TIMESTAMP_BUFFER_BYTES: u64 = MAX_TIMESTAMPS as u64 * BYTES_PER_U64;

/// Number of pipeline-statistics fields we request. Matches the bits
/// set in [`pipeline_stats_flags`]; resolve writes them back in the
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
fn pipeline_stats_flags() -> wgpu::PipelineStatisticsTypes {
    wgpu::PipelineStatisticsTypes::VERTEX_SHADER_INVOCATIONS
        | wgpu::PipelineStatisticsTypes::CLIPPER_INVOCATIONS
        | wgpu::PipelineStatisticsTypes::CLIPPER_PRIMITIVES_OUT
        | wgpu::PipelineStatisticsTypes::FRAGMENT_SHADER_INVOCATIONS
        | wgpu::PipelineStatisticsTypes::COMPUTE_SHADER_INVOCATIONS
}

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
}

#[derive(Debug)]
pub(crate) struct GpuTimings {
    /// `MAX_TIMESTAMPS` slots in per-batch mode, 2 in basic mode.
    timestamp_query_set: wgpu::QuerySet,
    /// Whether `TIMESTAMP_QUERY_INSIDE_PASSES` is available. False
    /// → only pass begin / end timestamps (via descriptor), no
    /// midpoint writes. Drives whether the caller invokes
    /// [`Self::pass_begin`] / [`Self::mark`] / [`Self::pass_end`]
    /// (yes) or relies on the descriptor's begin/end (no).
    pub(crate) inside_passes: bool,
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
    /// Shared sink for resolved samples. Backend owns the canonical
    /// `GpuPassStats` and clones a handle in here; consumers (Ui debug
    /// overlay, benches) hold their own clones of the same handle.
    sink: GpuPassStats,
}

impl GpuTimings {
    pub(crate) fn new(
        device: &wgpu::Device,
        period_ns: f32,
        inside_passes: bool,
        pipeline_stats: bool,
        sink: GpuPassStats,
    ) -> Self {
        // Timestamp query set sized for the more permissive mode.
        // Basic mode only uses indices 0 and 1, but the over-allocation
        // is 32 * 8 = 256 bytes, not worth a second code path.
        let timestamp_query_set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("aperture.gpu_timings.timestamps"),
            ty: wgpu::QueryType::Timestamp,
            count: MAX_TIMESTAMPS,
        });
        let timestamps_resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("aperture.gpu_timings.timestamps.resolve"),
            size: TIMESTAMP_BUFFER_BYTES,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let (stats_query_set, stats_resolve) = if pipeline_stats {
            let qs = device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("aperture.gpu_timings.stats"),
                ty: wgpu::QueryType::PipelineStatistics(pipeline_stats_flags()),
                count: 1,
            });
            let buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("aperture.gpu_timings.stats.resolve"),
                size: STATS_BUFFER_BYTES,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            (Some(qs), Some(buf))
        } else {
            (None, None)
        };

        let slots = std::array::from_fn(|_| Slot {
            timestamps_buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("aperture.gpu_timings.timestamps.staging"),
                size: TIMESTAMP_BUFFER_BYTES,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            timestamps_count: 0,
            segment_kinds: Vec::new(),
            stats_buffer: pipeline_stats.then(|| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("aperture.gpu_timings.stats.staging"),
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
            },
            sink,
        }
    }

    /// Descriptor for `RenderPassDescriptor::timestamp_writes` in basic
    /// mode. `None` when per-batch mode is active — there we write
    /// pass begin / end inline via `RenderPass::write_timestamp`
    /// instead, so we don't double-write index 0.
    pub(crate) fn pass_writes(&self) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
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
    /// Per-batch mode only. Called immediately after
    /// `begin_render_pass`.
    pub(crate) fn pass_begin(&self, pass: &mut wgpu::RenderPass<'_>) {
        assert!(self.inside_passes);
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

    /// Write the pass-end timestamp, closing the final segment.
    /// Per-batch mode only. Called immediately before the pass is
    /// dropped.
    pub(crate) fn pass_end(&self, pass: &mut wgpu::RenderPass<'_>) {
        assert!(self.inside_passes);
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

    /// Emit `resolve_query_set` + `copy_buffer_to_buffer` into the
    /// caller's encoder. Picks the first idle staging slot; if both
    /// are in-flight, drops this frame's measurement.
    pub(crate) fn resolve(&mut self, encoder: &mut wgpu::CommandEncoder) {
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
        let bytes = count as u64 * BYTES_PER_U64;
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
    /// [`gpu_pass_stats`].
    pub(crate) fn after_submit(&mut self, device: &wgpu::Device) {
        if let Some(slot_idx) = self.pending_slot.take() {
            let map_state = self.slots[slot_idx].map_state.clone();
            map_state.store(0, Release);
            self.slots[slot_idx].timestamps_buffer.slice(..).map_async(
                wgpu::MapMode::Read,
                move |res| {
                    let failed = if res.is_err() { TIMESTAMPS_FAILED } else { 0 };
                    map_state.fetch_or(TIMESTAMPS_DONE | failed, Release);
                },
            );
            if let Some(stats_buf) = &self.slots[slot_idx].stats_buffer {
                let map_state = self.slots[slot_idx].map_state.clone();
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
                consume_slot(slot, self.period_ns, &self.sink);
            }
            slot.map_state.store(0, Release);
            slot.in_flight = false;
        }
    }
}

fn mappings_complete(state: u8, has_stats: bool) -> bool {
    let required = TIMESTAMPS_DONE | if has_stats { STATS_DONE } else { 0 };
    state & required == required
}

fn mappings_failed(state: u8) -> bool {
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
/// Caller is responsible for clearing `in_flight` / `ready` after this
/// returns.
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
        let first = u64::from_le_bytes(ts[..8].try_into().unwrap());
        let last_off = (count - 1) * 8;
        let last = u64::from_le_bytes(ts[last_off..last_off + 8].try_into().unwrap());
        let delta_ns = (last.saturating_sub(first) as f64 * period_ns as f64) as u64;
        sink.record_pass_ns(delta_ns);
        sink.clear_kinds();
    }
    // Per-batch attribution when we collected midpoint marks.
    if count >= 3 {
        let mut per_kind_ns = [0u64; <BatchKind as strum::EnumCount>::COUNT];
        for i in 0..count - 1 {
            let t0_off = i * 8;
            let t1_off = (i + 1) * 8;
            let t0 = u64::from_le_bytes(ts[t0_off..t0_off + 8].try_into().unwrap());
            let t1 = u64::from_le_bytes(ts[t1_off..t1_off + 8].try_into().unwrap());
            let kind = segment_kinds.get(i).copied().unwrap_or(BatchKind::Setup);
            let seg_ns = (t1.saturating_sub(t0) as f64 * period_ns as f64) as u64;
            per_kind_ns[kind.idx()] = per_kind_ns[kind.idx()].saturating_add(seg_ns);
        }
        // Publish every kind explicitly, including zero-ns ones — a
        // category that ran but rounded to zero is still distinct
        // from one that didn't run at all (already cleared above).
        for kind in BatchKind::iter() {
            sink.record_kind_ns(kind, per_kind_ns[kind.idx()]);
        }
    }
}

/// Parse the resolved pipeline-statistics counters and publish them.
/// Field order matches `pipeline_stats_flags` — the mapping lives
/// here, next to the flag declaration that defines it.
fn publish_stats(bytes: &[u8], sink: &GpuPassStats) {
    let mut values = [0u64; STATS_FIELD_COUNT];
    for (i, v) in values.iter_mut().enumerate() {
        let off = i * 8;
        *v = u64::from_le_bytes(bytes[off..off + 8].try_into().unwrap());
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

    fn ts_bytes(ticks: &[u64]) -> Vec<u8> {
        ticks.iter().flat_map(|t| t.to_le_bytes()).collect()
    }

    #[test]
    fn blank_measured_frame_clears_stale_per_kind_stats() {
        let sink = GpuPassStats::default();

        // Frame 1 (per-batch): timestamps [1000, 3000, 6000] ticks at
        // 1 ns/tick, segments [Quads, Text]:
        //   pass  = 6000 - 1000 = 5000 ns
        //   quads = 3000 - 1000 = 2000 ns
        //   text  = 6000 - 3000 = 3000 ns
        publish_timestamps(
            &ts_bytes(&[1000, 3000, 6000]),
            3,
            &[BatchKind::Quads, BatchKind::Text],
            1.0,
            &sink,
        );
        assert_eq!(sink.last_pass_ms(), Some(0.005));
        assert_eq!(sink.last_kind_ms(BatchKind::Quads), Some(0.002));
        assert_eq!(sink.last_kind_ms(BatchKind::Text), Some(0.003));
        // Kinds without a segment still publish, as exactly zero.
        assert_eq!(sink.last_kind_ms(BatchKind::Mesh), Some(0.0));

        // Frame 2: begin/end only (count == 2 — a truly blank window
        // in per-batch mode). Pass time refreshes to 14000 - 10000 =
        // 4000 ns; every per-kind slot clears to None instead of
        // keeping frame 1's values.
        publish_timestamps(&ts_bytes(&[10_000, 14_000]), 2, &[], 1.0, &sink);
        assert_eq!(sink.last_pass_ms(), Some(0.004));
        for kind in BatchKind::iter() {
            assert_eq!(
                sink.last_kind_ms(kind),
                None,
                "{} stale after blank measured frame",
                kind.label(),
            );
        }
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
