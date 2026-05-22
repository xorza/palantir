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
//! handle (e.g. via `Host::gpu_pass_stats`).

use super::gpu_pass_stats::{BatchKind, GpuPassStats};
use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
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

/// All pipeline-statistics fields we care about. Compute is included
/// for layout completeness (always 0 — we have no compute passes).
fn pipeline_stats_flags() -> wgpu::PipelineStatisticsTypes {
    wgpu::PipelineStatisticsTypes::VERTEX_SHADER_INVOCATIONS
        | wgpu::PipelineStatisticsTypes::CLIPPER_INVOCATIONS
        | wgpu::PipelineStatisticsTypes::CLIPPER_PRIMITIVES_OUT
        | wgpu::PipelineStatisticsTypes::FRAGMENT_SHADER_INVOCATIONS
        | wgpu::PipelineStatisticsTypes::COMPUTE_SHADER_INVOCATIONS
}

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
    ready: Arc<AtomicBool>,
    in_flight: bool,
}

/// Interior-mutable per-frame state. The render-pass walk in
/// `WgpuBackend::render_groups` holds `&self` on the whole backend, so
/// it can't mutate `GpuTimings` directly — these cells let `mark()`
/// bump the timestamp index and append to `segment_kinds` through a
/// shared reference.
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

pub(super) struct GpuTimings {
    /// `MAX_TIMESTAMPS` slots in per-batch mode, 2 in basic mode.
    timestamp_query_set: wgpu::QuerySet,
    /// Whether `TIMESTAMP_QUERY_INSIDE_PASSES` is available. False
    /// → only pass begin / end timestamps (via descriptor), no
    /// midpoint writes.
    inside_passes: bool,
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
    pub(super) fn new(
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
            label: Some("palantir.gpu_timings.timestamps"),
            ty: wgpu::QueryType::Timestamp,
            count: MAX_TIMESTAMPS,
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
                ty: wgpu::QueryType::PipelineStatistics(pipeline_stats_flags()),
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

        let slots = std::array::from_fn(|_| Slot {
            timestamps_buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("palantir.gpu_timings.timestamps.staging"),
                size: TIMESTAMP_BUFFER_BYTES,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            timestamps_count: 0,
            segment_kinds: Vec::new(),
            stats_buffer: pipeline_stats.then(|| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("palantir.gpu_timings.stats.staging"),
                    size: STATS_BUFFER_BYTES,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            }),
            ready: Arc::new(AtomicBool::new(false)),
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
    pub(super) fn pass_writes(&self) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        if self.inside_passes {
            return None;
        }
        Some(wgpu::RenderPassTimestampWrites {
            query_set: &self.timestamp_query_set,
            beginning_of_pass_write_index: Some(0),
            end_of_pass_write_index: Some(1),
        })
    }

    /// Whether `TIMESTAMP_QUERY_INSIDE_PASSES` is on. Drives whether
    /// the caller invokes [`Self::pass_begin`] / [`Self::mark`] /
    /// [`Self::pass_end`] (yes) or relies on the descriptor's begin/end
    /// (no).
    pub(super) fn inside_passes(&self) -> bool {
        self.inside_passes
    }

    /// Reset per-frame state, then write the pass-begin timestamp.
    /// Per-batch mode only. Called immediately after
    /// `begin_render_pass`.
    pub(super) fn pass_begin(&self, pass: &mut wgpu::RenderPass<'_>) {
        debug_assert!(self.inside_passes);
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
    pub(super) fn mark(&self, pass: &mut wgpu::RenderPass<'_>, kind: BatchKind) {
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
    pub(super) fn pass_end(&self, pass: &mut wgpu::RenderPass<'_>) {
        debug_assert!(self.inside_passes);
        let idx = self.inner.next_index.get();
        pass.write_timestamp(&self.timestamp_query_set, idx);
        let final_kind = self.inner.current_kind.get().unwrap_or(BatchKind::Setup);
        self.inner.segment_kinds.borrow_mut().push(final_kind);
        self.inner.next_index.set(idx + 1);
    }

    /// Start the pipeline-statistics query around the pass. No-op when
    /// the feature is off.
    pub(super) fn begin_pipeline_stats(&self, pass: &mut wgpu::RenderPass<'_>) {
        if let Some(qs) = &self.stats_query_set {
            pass.begin_pipeline_statistics_query(qs, 0);
        }
    }

    /// End the pipeline-statistics query. No-op when the feature is off.
    pub(super) fn end_pipeline_stats(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.stats_query_set.is_some() {
            pass.end_pipeline_statistics_query();
        }
    }

    /// Emit `resolve_query_set` + `copy_buffer_to_buffer` into the
    /// caller's encoder. Picks the first idle staging slot; if both
    /// are in-flight, drops this frame's measurement.
    pub(super) fn resolve(&mut self, encoder: &mut wgpu::CommandEncoder) {
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
        // Move per-frame segment-kind labels into the slot for the
        // async readback path. Cheap — typically <16 entries.
        let mut slot_kinds = std::mem::take(&mut self.slots[slot].segment_kinds);
        slot_kinds.clear();
        slot_kinds.extend(self.inner.segment_kinds.borrow().iter().copied());
        self.slots[slot].segment_kinds = slot_kinds;

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
    pub(super) fn after_submit(&mut self, device: &wgpu::Device) {
        if let Some(slot_idx) = self.pending_slot.take() {
            let ready = self.slots[slot_idx].ready.clone();
            // Map the timestamps buffer; once it fires we'll also read
            // the (already-completed) stats buffer in the same poll
            // tick.
            self.slots[slot_idx].timestamps_buffer.slice(..).map_async(
                wgpu::MapMode::Read,
                move |res| {
                    if res.is_ok() {
                        ready.store(true, Relaxed);
                    }
                },
            );
            if let Some(stats_buf) = &self.slots[slot_idx].stats_buffer {
                // Independent map, ignored result — the timestamps
                // `ready` is the gate; the stats buffer races toward
                // mapped on the same submit so by the time we check
                // both, both are mapped. If the stats map fails (e.g.
                // device loss) `get_mapped_range` below will panic,
                // which is fine in instrumentation-only code paths.
                stats_buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
            }
        }
        let _ = device.poll(wgpu::PollType::Poll);
        for slot in &mut self.slots {
            if slot.in_flight && slot.ready.load(Relaxed) {
                consume_slot(slot, self.period_ns, &self.sink);
                slot.ready.store(false, Relaxed);
                slot.in_flight = false;
            }
        }
    }
}

/// Read the mapped buffers on `slot`, publish into `sink`, then unmap.
/// Caller is responsible for clearing `in_flight` / `ready` after this
/// returns.
fn consume_slot(slot: &mut Slot, period_ns: f32, sink: &GpuPassStats) {
    let ts_range = slot.timestamps_buffer.slice(..).get_mapped_range();
    let count = slot.timestamps_count as usize;
    // Always: pass duration = last - first.
    if count >= 2 {
        let first = u64::from_le_bytes(ts_range[..8].try_into().unwrap());
        let last_off = (count - 1) * 8;
        let last = u64::from_le_bytes(ts_range[last_off..last_off + 8].try_into().unwrap());
        let delta_ns = (last.saturating_sub(first) as f64 * period_ns as f64) as u64;
        sink.record_pass_ns(delta_ns);
    }
    // Per-batch attribution when we collected midpoint marks. Clear
    // the kind table first so categories that didn't run this frame
    // surface as `None` rather than stale prior-frame values.
    if count >= 3 {
        sink.clear_kinds();
        let mut per_kind_ns = [0u64; <BatchKind as strum::EnumCount>::COUNT];
        for i in 0..count - 1 {
            let t0_off = i * 8;
            let t1_off = (i + 1) * 8;
            let t0 = u64::from_le_bytes(ts_range[t0_off..t0_off + 8].try_into().unwrap());
            let t1 = u64::from_le_bytes(ts_range[t1_off..t1_off + 8].try_into().unwrap());
            let kind = slot
                .segment_kinds
                .get(i)
                .copied()
                .unwrap_or(BatchKind::Setup);
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
    drop(ts_range);
    slot.timestamps_buffer.unmap();

    if let Some(stats_buf) = &slot.stats_buffer {
        let s_range = stats_buf.slice(..).get_mapped_range();
        let mut values = [0u64; 5];
        for (i, v) in values.iter_mut().enumerate() {
            let off = i * 8;
            *v = u64::from_le_bytes(s_range[off..off + 8].try_into().unwrap());
        }
        drop(s_range);
        stats_buf.unmap();
        sink.record_pipeline_stats(values);
    }
}
