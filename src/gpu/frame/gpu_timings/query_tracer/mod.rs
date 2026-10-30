//! The tracer of a device that writes timestamps: `wgpu` timestamp and pipeline-statistics queries (see `DeviceRequirements::GPU_TIMING_FEATURES`).
//!
//! - `TIMESTAMP_QUERY_INSIDE_PASSES` writes a timestamp at every category transition in `render_groups`, attributed per [`BatchKind`]; off, only pass begin/end are timed. Where the main pass breaks at a pane, the break's start and end are timestamped, and the captures and blurs between count in no category.
//! - `PIPELINE_STATISTICS_QUERY` brackets each pass of the main pass, and the frame publishes their sum.
//! - `TIMESTAMP_QUERY_INSIDE_ENCODERS` brackets the backbuffer copy-out at [`COPY_OUT_IDX`], published as [`GpuPassStats::last_copy_out`], and each backdrop break's captures and blurs at [`BACKDROP_IDX`], published as their sums.
//!
//! A frame's queries resolve in a later frame, once the queue said the frame's work is done, and
//! read back after that frame completes: publishing lags two frames. A Metal device on macOS 26
//! and later reads a timestamp resolved in the command buffer that wrote it, or in one submitted
//! before the CPU learned it completed, as the value written before it (wgpu #9414); resolving
//! after the work-done callback reads right on every backend.

use crate::diagnostics::gpu_pass_stats::{BackdropTimes, BatchKind, GpuPassStats, PipelineStats};
use crate::gpu::device::gpu::Gpu;
use crate::gpu::frame::gpu_timings::{BackdropPhase, Mark};
use crate::gpu::frame::main_segment::MainSegment;
use std::array;
use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering::Acquire, Ordering::Release};

const BYTES_PER_U64: u64 = 8;

/// Max timestamps per frame: two reserved for pass begin/end, the rest for per-batch transitions; excess transitions fold into the surrounding category.
const MAX_TIMESTAMPS: u32 = 32;

/// Where the copy-out's timestamps sit, past every main-pass slot; a query resolve must land on a `QUERY_RESOLVE_BUFFER_ALIGNMENT` offset, which the main pass's slots fill exactly.
const COPY_OUT_IDX: u32 = MAX_TIMESTAMPS;
const COPY_OUT_OFFSET: u64 = COPY_OUT_IDX as u64 * BYTES_PER_U64;
/// The backdrop breaks whose captures and blurs are timed; a frame's later breaks go untimed.
const MAX_TIMED_BREAKS: u32 = 8;
/// Where the breaks' timestamps start, at the resolve alignment past the copy-out's: three each,
/// the captures' start, their end, which starts the blurs, and the blurs' end.
const BACKDROP_IDX: u32 = 2 * MAX_TIMESTAMPS;
const BACKDROP_OFFSET: u64 = BACKDROP_IDX as u64 * BYTES_PER_U64;
const QUERY_COUNT: u32 = BACKDROP_IDX + 3 * MAX_TIMED_BREAKS;
const TIMESTAMP_BUFFER_BYTES: u64 = QUERY_COUNT as u64 * BYTES_PER_U64;

const _: () = assert!(COPY_OUT_OFFSET.is_multiple_of(wgpu::QUERY_RESOLVE_BUFFER_ALIGNMENT));
const _: () = assert!(BACKDROP_OFFSET.is_multiple_of(wgpu::QUERY_RESOLVE_BUFFER_ALIGNMENT));
const _: () = assert!(COPY_OUT_IDX + 2 <= BACKDROP_IDX);

const STATS_FIELD_COUNT: usize = 5;
const STATS_QUERY_BYTES: u64 = STATS_FIELD_COUNT as u64 * BYTES_PER_U64;
/// The passes of a main pass whose pipeline statistics a frame sums; a frame broken into more
/// publishes none.
const MAX_STATS_PASSES: u32 = 16;
const STATS_BUFFER_BYTES: u64 = MAX_STATS_PASSES as u64 * STATS_QUERY_BYTES;

/// Slots in rotation: one the recorded frame writes, one whose frame waits for the queue, one whose
/// resolve maps; with every slot busy a frame goes unmeasured.
const SLOTS: usize = 3;

/// A slot's staging buffers hold no map in flight.
const UNMAPPED: u8 = 0;
/// A map is in flight; the bits below land on it.
const MAPPING: u8 = 1 << 4;
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

/// Where a slot's frame stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SlotState {
    /// Holds nothing; the next frame may write into it.
    Free,
    /// The frame being recorded writes into its query sets.
    Writing,
    /// Its frame was submitted; it resolves once the queue said the frame's work is done.
    Submitted,
    /// Its resolve was recorded in a later frame, and its staging buffers map.
    Resolving,
}

/// One frame's queries, from the frame that writes them to the readback that publishes them.
#[derive(Debug)]
struct Slot {
    query_set: wgpu::QuerySet,
    /// The pipeline-statistics query, when the device has `PIPELINE_STATISTICS_QUERY`.
    stats_query_set: Option<wgpu::QuerySet>,
    timestamps_buffer: wgpu::Buffer,
    timestamps_count: u32,
    /// Kind of the segment between timestamps `i` and `i+1`, saved at `finish()`, `None` for a
    /// break between passes; empty in basic mode.
    segment_kinds: Vec<Option<BatchKind>>,
    copied_out: bool,
    /// The backdrop breaks whose timestamps the slot holds.
    backdrop_breaks: u32,
    /// The passes whose pipeline statistics the slot holds; `None` when the frame had more than
    /// [`MAX_STATS_PASSES`].
    stats_passes: Option<u32>,
    stats_buffer: Option<wgpu::Buffer>,
    map_state: Arc<AtomicU8>,
    /// Set by the queue's work-done callback once the frame that wrote the slot completed.
    done: Arc<AtomicBool>,
    state: SlotState,
}

/// Interior-mutable per-frame state: `render_groups` holds `&self`, so `mark()` mutates through these.
#[derive(Debug)]
struct Inner {
    next_idx: Cell<u32>,
    /// Kind in flight since the last timestamp; `None` before the first `mark()` (recorded as [`BatchKind::Setup`]).
    cur_kind: Cell<Option<BatchKind>>,
    segment_kinds: RefCell<Vec<Option<BatchKind>>>,
    /// The last pass ended at a break, whose end the next pass's start timestamps.
    broken: Cell<bool>,
    /// A break found no room for its timestamps, so the frame attributes no time by kind.
    kinds_void: Cell<bool>,
    copied_out: Cell<bool>,
    /// The backdrop breaks timed this frame.
    backdrop_breaks: Cell<u32>,
    /// The passes that opened a pipeline-statistics query this frame, counting those past
    /// [`MAX_STATS_PASSES`].
    stats_passes: Cell<u32>,
}

#[derive(Debug)]
pub(super) struct QueryTracer {
    /// Whether `TIMESTAMP_QUERY_INSIDE_PASSES` is available; off, [`Self::segment_begin`], [`Self::mark`] and [`Self::segment_end`] write no timestamps.
    inside_passes: bool,
    /// Whether `TIMESTAMP_QUERY_INSIDE_ENCODERS` is available; off, the copy-out is not timed.
    inside_encoders: bool,
    timestamps_resolve: wgpu::Buffer,
    stats_resolve: Option<wgpu::Buffer>,
    slots: [Slot; SLOTS],
    /// The slot the frame being recorded writes into; `None` while every slot is busy, which
    /// leaves the frame unmeasured.
    writing: Option<usize>,
    /// The slot [`Self::finish`] closed, which `after_submit` hands the queue's work-done callback.
    submitted: Option<usize>,
    period_ns: f32,
    inner: Inner,
}

impl QueryTracer {
    /// Timings at `period_ns` per tick, attributed as finely as `features` allow.
    pub(super) fn new(device: &wgpu::Device, period_ns: f32, features: wgpu::Features) -> Self {
        let inside_passes = features.contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES);
        let inside_encoders = features.contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS);
        let pipeline_stats = features.contains(wgpu::Features::PIPELINE_STATISTICS_QUERY);
        let resolve = |label, size| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            })
        };
        let staging = |label, size| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let slots = array::from_fn(|i| Slot {
            query_set: device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("palantir.gpu_timings.timestamps"),
                ty: wgpu::QueryType::Timestamp,
                count: QUERY_COUNT,
            }),
            stats_query_set: pipeline_stats.then(|| {
                device.create_query_set(&wgpu::QuerySetDescriptor {
                    label: Some("palantir.gpu_timings.stats"),
                    ty: wgpu::QueryType::PipelineStatistics(PIPELINE_STATS_FLAGS),
                    count: MAX_STATS_PASSES,
                })
            }),
            timestamps_buffer: staging(
                "palantir.gpu_timings.timestamps.staging",
                TIMESTAMP_BUFFER_BYTES,
            ),
            timestamps_count: 0,
            segment_kinds: Vec::with_capacity(MAX_TIMESTAMPS as usize),
            copied_out: false,
            backdrop_breaks: 0,
            stats_passes: Some(0),
            stats_buffer: pipeline_stats
                .then(|| staging("palantir.gpu_timings.stats.staging", STATS_BUFFER_BYTES)),
            map_state: Arc::new(AtomicU8::new(UNMAPPED)),
            done: Arc::new(AtomicBool::new(false)),
            state: if i == 0 {
                SlotState::Writing
            } else {
                SlotState::Free
            },
        });
        Self {
            inside_passes,
            inside_encoders,
            timestamps_resolve: resolve(
                "palantir.gpu_timings.timestamps.resolve",
                TIMESTAMP_BUFFER_BYTES,
            ),
            stats_resolve: pipeline_stats
                .then(|| resolve("palantir.gpu_timings.stats.resolve", STATS_BUFFER_BYTES)),
            slots,
            writing: Some(0),
            submitted: None,
            period_ns,
            inner: Inner {
                next_idx: Cell::new(0),
                cur_kind: Cell::new(None),
                segment_kinds: RefCell::new(Vec::with_capacity(MAX_TIMESTAMPS as usize)),
                broken: Cell::new(false),
                kinds_void: Cell::new(false),
                copied_out: Cell::new(false),
                backdrop_breaks: Cell::new(0),
                stats_passes: Cell::new(0),
            },
        }
    }

    /// The query set the frame being recorded writes into, `None` while it goes unmeasured.
    fn query_set(&self) -> Option<&wgpu::QuerySet> {
        self.writing.map(|slot| &self.slots[slot].query_set)
    }

    /// Descriptor for `timestamp_writes` in basic mode, for the main pass's `segment`: the first
    /// pass writes the begin and the last the end, so the time spans the captures between them.
    /// `None` in per-batch mode, which writes begin/end inline, and on a pass between two others.
    pub(super) fn pass_writes(
        &self,
        segment: MainSegment,
    ) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        if self.inside_passes || !(segment.opens() || segment.closes()) {
            return None;
        }
        Some(wgpu::RenderPassTimestampWrites {
            query_set: self.query_set()?,
            beginning_of_pass_write_index: segment.opens().then_some(0),
            end_of_pass_write_index: segment.closes().then_some(1),
        })
    }

    /// Open `segment`'s timing, right after `begin_render_pass`: the main pass's first timestamp
    /// where it opens, else the end of the break before it, and the pass's pipeline statistics.
    pub(super) fn segment_begin(&self, pass: &mut wgpu::RenderPass<'_>, segment: MainSegment) {
        if segment.opens() {
            self.inner.next_idx.set(0);
            self.inner.cur_kind.set(None);
            self.inner.segment_kinds.borrow_mut().clear();
            self.inner.broken.set(false);
            self.inner.kinds_void.set(false);
        }
        self.begin_pipeline_stats(pass);
        let Some(set) = self.query_set().filter(|_| self.inside_passes) else {
            return;
        };
        if segment.opens() {
            pass.write_timestamp(set, 0);
            self.inner.next_idx.set(1);
        } else if self.inner.broken.replace(false) {
            self.write_segment_end(pass, set, None);
        }
    }

    /// Marks a category boundary: no-op if `kind` is active, else writes a timestamp and records the closed segment. Once the set is full minus the pass-end slot, transitions fold into the current kind.
    pub(super) fn mark(&self, pass: &mut wgpu::RenderPass<'_>, kind: BatchKind) {
        let Some(set) = self.query_set().filter(|_| self.inside_passes) else {
            return;
        };
        let cur = self.inner.cur_kind.get();
        if cur == Some(kind) {
            return;
        }
        let idx = self.inner.next_idx.get();
        if idx >= MAX_TIMESTAMPS - 1 {
            return;
        }
        self.write_segment_end(pass, set, Some(cur.unwrap_or(BatchKind::Setup)));
        self.inner.cur_kind.set(Some(kind));
    }

    /// Close `segment`'s timing: the main pass's last timestamp where it closes, else the start of
    /// the break after it, while the set holds room for the break's end and the pass's last.
    pub(super) fn segment_end(&self, pass: &mut wgpu::RenderPass<'_>, segment: MainSegment) {
        self.end_pipeline_stats(pass);
        let Some(set) = self.query_set().filter(|_| self.inside_passes) else {
            return;
        };
        let kind = Some(self.inner.cur_kind.get().unwrap_or(BatchKind::Setup));
        if segment.closes() {
            self.write_segment_end(pass, set, kind);
        } else if self.inner.next_idx.get() + 2 < MAX_TIMESTAMPS {
            self.write_segment_end(pass, set, kind);
            self.inner.broken.set(true);
        } else {
            self.inner.kinds_void.set(true);
        }
    }

    /// Timestamp the end of the segment since the last timestamp, of `kind`.
    fn write_segment_end(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        set: &wgpu::QuerySet,
        kind: Option<BatchKind>,
    ) {
        let idx = self.inner.next_idx.get();
        pass.write_timestamp(set, idx);
        self.inner.segment_kinds.borrow_mut().push(kind);
        self.inner.next_idx.set(idx + 1);
    }

    /// Open this pass's pipeline-statistics query, while the frame has one left.
    fn begin_pipeline_stats(&self, pass: &mut wgpu::RenderPass<'_>) {
        let idx = self.inner.stats_passes.get();
        self.inner.stats_passes.set(idx + 1);
        if let Some(set) = self.stats_query_set().filter(|_| idx < MAX_STATS_PASSES) {
            pass.begin_pipeline_statistics_query(set, idx);
        }
    }

    /// Close the query [`Self::begin_pipeline_stats`] opened, if it opened one.
    fn end_pipeline_stats(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.stats_query_set().is_some() && self.inner.stats_passes.get() <= MAX_STATS_PASSES {
            pass.end_pipeline_statistics_query();
        }
    }

    /// The pipeline-statistics query the frame being recorded writes into, if any.
    fn stats_query_set(&self) -> Option<&wgpu::QuerySet> {
        self.slots[self.writing?].stats_query_set.as_ref()
    }

    /// Write `mark`'s timestamp between passes; nothing on a device that writes none there.
    pub(super) fn encoder_mark(&self, encoder: &mut wgpu::CommandEncoder, mark: Mark) {
        let Some(set) = self.query_set().filter(|_| self.inside_encoders) else {
            return;
        };
        let idx = match mark {
            // A run's span publishes as the pass time, from the pass's two slots.
            #[cfg(feature = "internals")]
            Mark::RunStart => 0,
            #[cfg(feature = "internals")]
            Mark::RunEnd => 1,
            Mark::CopyOutStart => COPY_OUT_IDX,
            Mark::CopyOutEnd => {
                self.inner.copied_out.set(true);
                COPY_OUT_IDX + 1
            }
            Mark::Backdrop { at, phase } => {
                let Some(at) = u32::try_from(at).ok().filter(|&b| b < MAX_TIMED_BREAKS) else {
                    return;
                };
                if phase == BackdropPhase::BlurEnd {
                    self.inner.backdrop_breaks.set(at + 1);
                }
                BACKDROP_IDX + 3 * at + phase as u32
            }
        };
        encoder.write_timestamp(set, idx);
    }

    /// Close the frame `encoder` recorded: its slot waits for the queue to say the frame is done,
    /// and every slot whose frame the queue already said is done resolves in this encoder. A
    /// Metal device reads a timestamp resolved before the CPU learned its frame completed as the
    /// value before it, so no slot resolves in the frame that wrote it. The next frame writes into
    /// a free slot, or goes unmeasured while every slot is busy.
    pub(super) fn finish(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let copied_out = self.inner.copied_out.replace(false);
        let backdrop_breaks = self.inner.backdrop_breaks.replace(0);
        let stats_passes = self.inner.stats_passes.replace(0);
        if let Some(at) = self.writing.take() {
            // Basic mode is 2 (the descriptor wrote 0/1); per-batch is what the segments and `mark()` accumulated.
            let count = if self.inside_passes {
                self.inner.next_idx.get().clamp(2, MAX_TIMESTAMPS)
            } else {
                2
            };
            let slot = &mut self.slots[at];
            slot.timestamps_count = count;
            slot.segment_kinds.clear();
            let kinds = self.inner.segment_kinds.borrow();
            if self.inner.kinds_void.get() {
                slot.segment_kinds.resize(kinds.len(), None);
            } else {
                slot.segment_kinds.extend(kinds.iter().copied());
            }
            drop(kinds);
            slot.stats_passes = (stats_passes <= MAX_STATS_PASSES).then_some(stats_passes);
            slot.copied_out = copied_out;
            slot.backdrop_breaks = backdrop_breaks;
            slot.done.store(false, Release);
            slot.state = SlotState::Submitted;
            self.submitted = Some(at);
        }
        self.resolve_done(encoder);
        self.writing = self.free_slot();
    }

    /// Record the resolve of every slot whose frame the queue said is done into `encoder`.
    pub(super) fn resolve_done(&mut self, encoder: &mut wgpu::CommandEncoder) {
        for slot in &mut self.slots {
            if slot.state != SlotState::Submitted || !slot.done.load(Acquire) {
                continue;
            }
            slot.resolve(
                encoder,
                &self.timestamps_resolve,
                self.stats_resolve.as_ref(),
            );
            slot.state = SlotState::Resolving;
        }
    }

    /// Mark a free slot as the next frame's, `None` while every slot is busy.
    fn free_slot(&mut self) -> Option<usize> {
        let at = self
            .slots
            .iter()
            .position(|slot| slot.state == SlotState::Free)?;
        self.slots[at].state = SlotState::Writing;
        Some(at)
    }

    /// After `queue.submit` on `gpu`: the frame [`Self::finish`] closed waits for the queue's
    /// work-done callback, the slots resolved in it start to map, and each slot whose readback
    /// landed publishes into `sink` and frees.
    pub(super) fn after_submit(&mut self, gpu: &Gpu, sink: &GpuPassStats) {
        if let Some(at) = self.submitted.take() {
            let done = Arc::clone(&self.slots[at].done);
            gpu.queue
                .on_submitted_work_done(move || done.store(true, Release));
        }
        for slot in &mut self.slots {
            if slot.state == SlotState::Resolving && slot.map_state.load(Acquire) == UNMAPPED {
                slot.map();
            }
        }
        let publisher = Publisher {
            period_ns: self.period_ns,
            sink,
        };
        for slot in &mut self.slots {
            if slot.state != SlotState::Resolving {
                continue;
            }
            let state = slot.map_state.load(Acquire);
            if !mappings_complete(state, slot.stats_buffer.is_some()) {
                continue;
            }
            if mappings_failed(state) {
                slot.discard(state);
            } else {
                slot.consume(publisher);
            }
            slot.map_state.store(UNMAPPED, Release);
            slot.state = SlotState::Free;
        }
        if self.writing.is_none() {
            self.writing = self.free_slot();
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

impl Slot {
    /// Record this slot's resolve into `encoder`: its timestamps through `timestamps_resolve`
    /// and its statistics through `stats_resolve`, onto its staging buffers.
    fn resolve(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        timestamps_resolve: &wgpu::Buffer,
        stats_resolve: Option<&wgpu::Buffer>,
    ) {
        let mut copy = |range: Range<u32>, offset: u64| {
            encoder.resolve_query_set(&self.query_set, range.clone(), timestamps_resolve, offset);
            encoder.copy_buffer_to_buffer(
                timestamps_resolve,
                offset,
                &self.timestamps_buffer,
                offset,
                u64::from(range.end - range.start) * BYTES_PER_U64,
            );
        };
        copy(0..self.timestamps_count, 0);
        if self.copied_out {
            copy(COPY_OUT_IDX..COPY_OUT_IDX + 2, COPY_OUT_OFFSET);
        }
        if self.backdrop_breaks > 0 {
            copy(
                BACKDROP_IDX..BACKDROP_IDX + 3 * self.backdrop_breaks,
                BACKDROP_OFFSET,
            );
        }
        if let (Some(set), Some(resolve), Some(staging), Some(passes)) = (
            &self.stats_query_set,
            stats_resolve,
            &self.stats_buffer,
            self.stats_passes.filter(|&passes| passes > 0),
        ) {
            encoder.resolve_query_set(set, 0..passes, resolve, 0);
            encoder.copy_buffer_to_buffer(
                resolve,
                0,
                staging,
                0,
                u64::from(passes) * STATS_QUERY_BYTES,
            );
        }
    }

    /// Start mapping this slot's staging buffers, which land once the submit that resolved them
    /// completed.
    fn map(&self) {
        let map_state = Arc::clone(&self.map_state);
        map_state.store(MAPPING, Release);
        self.timestamps_buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |res| {
                let failed = if res.is_err() { TIMESTAMPS_FAILED } else { 0 };
                map_state.fetch_or(TIMESTAMPS_DONE | failed, Release);
            });
        if let Some(stats_buf) = &self.stats_buffer {
            let map_state = Arc::clone(&self.map_state);
            stats_buf
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |res| {
                    let failed = if res.is_err() { STATS_FAILED } else { 0 };
                    map_state.fetch_or(STATS_DONE | failed, Release);
                });
        }
    }

    /// Unmap whatever mapped, after either mapping failed.
    fn discard(&self, state: u8) {
        if state & TIMESTAMPS_FAILED == 0 {
            self.timestamps_buffer.unmap();
        }
        if state & STATS_FAILED == 0
            && let Some(stats_buf) = &self.stats_buffer
        {
            stats_buf.unmap();
        }
    }

    /// Publish this slot's landed readback and unmap it.
    fn consume(&self, publisher: Publisher<'_>) {
        let ts_range = self
            .timestamps_buffer
            .slice(..)
            .get_mapped_range()
            .expect("map timestamps range");
        publisher.timestamps(
            &ts_range,
            &self.segment_kinds,
            self.timestamps_count as usize,
        );
        publisher.copy_out(&ts_range, self.copied_out);
        publisher.backdrops(&ts_range, self.backdrop_breaks);
        drop(ts_range);
        self.timestamps_buffer.unmap();

        if let Some(stats_buf) = &self.stats_buffer {
            let s_range = stats_buf
                .slice(..)
                .get_mapped_range()
                .expect("map stats range");
            publisher.stats(&s_range, self.stats_passes);
            drop(s_range);
            stats_buf.unmap();
        }
    }
}

/// Where a landed readback goes, and the tick length that turns it into time; split from
/// [`Slot::consume`] to test without wgpu buffers.
#[derive(Clone, Copy, Debug)]
struct Publisher<'a> {
    period_ns: f32,
    sink: &'a GpuPassStats,
}

impl Publisher<'_> {
    /// Parses `count` resolved timestamps and publishes pass and per-kind durations; a segment of
    /// no kind counts in none.
    fn timestamps(self, ts: &[u8], segment_kinds: &[Option<BatchKind>], count: usize) {
        // Pass duration = last - first. The per-kind table clears on every measured frame, so a begin/end-only frame doesn't keep stale values.
        if count >= 2 {
            self.sink.record_pass_ns(self.span_ns(ts, 0, count - 1));
            self.sink.clear_kinds();
        }
        if count >= 3 {
            let mut per_kind_ns = [0u64; BatchKind::COUNT];
            let mut seen = [false; BatchKind::COUNT];
            for i in 0..count - 1 {
                let Some(kind) = segment_kinds.get(i).copied().flatten() else {
                    continue;
                };
                let seg_ns = self.span_ns(ts, i, i + 1);
                seen[kind.idx()] = true;
                per_kind_ns[kind.idx()] = per_kind_ns[kind.idx()].saturating_add(seg_ns);
            }
            for kind in BatchKind::ALL {
                if seen[kind.idx()] {
                    self.sink.record_kind_ns(kind, per_kind_ns[kind.idx()]);
                }
            }
        }
    }

    fn copy_out(self, ts: &[u8], copied_out: bool) {
        let begin = COPY_OUT_IDX as usize;
        let ns = copied_out.then(|| self.span_ns(ts, begin, begin + 1));
        self.sink.record_copy_out_ns(ns);
    }

    /// The sums of `breaks` backdrop breaks' capture and blur times; a frame with none clears
    /// both.
    fn backdrops(self, ts: &[u8], breaks: u32) {
        let at = |at: u32, phase: u32| (BACKDROP_IDX + 3 * at + phase) as usize;
        let sum = |from: u32, to: u32| {
            (0..breaks)
                .map(|b| self.span_ns(ts, at(b, from), at(b, to)))
                .sum::<u64>()
        };
        let timed = breaks > 0;
        self.sink.record_backdrop_ns(BackdropTimes {
            captures: timed.then(|| sum(0, 1)),
            blurs: timed.then(|| sum(1, 2)),
        });
    }

    /// The sums of `passes` passes' counters, in `wgpu::PipelineStatisticsTypes` order; `None`
    /// publishes none, for a frame of more passes than the queries hold.
    fn stats(self, bytes: &[u8], passes: Option<u32>) {
        let Some(passes) = passes else {
            self.sink.record_pipeline_stats(None);
            return;
        };
        let mut values = [0u64; STATS_FIELD_COUNT];
        for pass in 0..passes as usize {
            for (i, v) in values.iter_mut().enumerate() {
                *v = v.saturating_add(tick(bytes, pass * STATS_FIELD_COUNT + i));
            }
        }
        self.sink.record_pipeline_stats(Some(PipelineStats {
            vertex_shader_invocations: values[0],
            clipper_invocations: values[1],
            clipper_primitives_out: values[2],
            fragment_shader_invocations: values[3],
            compute_shader_invocations: values[4],
        }));
    }

    #[expect(
        clippy::cast_sign_loss,
        reason = "a tick delta is a saturating difference and the period is positive, so the product is never negative"
    )]
    fn span_ns(self, ts: &[u8], from: usize, to: usize) -> u64 {
        (tick(ts, to).saturating_sub(tick(ts, from)) as f64 * f64::from(self.period_ns)) as u64
    }
}

/// The `idx`th 64-bit word of a resolved query buffer. `pod_read_unaligned` rather than `bytemuck::cast_slice`: a mapped range's alignment is the driver's to promise, and a panicking cast would take the frame down over a debug counter.
#[inline]
fn tick(bytes: &[u8], idx: usize) -> u64 {
    let off = idx * 8;
    bytemuck::pod_read_unaligned(&bytes[off..off + 8])
}

#[cfg(test)]
mod tests;
