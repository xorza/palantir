//! Turning a frame of paint calls into the buffer the backend draws.
//!
//! [`Composer`] owns the scratch a compose pass is built in; a
//! [`ComposeSession`] is one pass, holding that scratch together with the
//! buffer it fills, and [`geometry`] is the arithmetic both are cut with.

use crate::display::Display;
use crate::renderer::frontend::composer::clip_stack::ClipStack;
use crate::renderer::frontend::composer::higher_kind::HigherKindRects;
use crate::renderer::frontend::composer::occlusion::OcclusionPruner;
use crate::renderer::frontend::composer::rect_grid::RectGrid;
use crate::renderer::frontend::composer::session::ComposeSession;
use crate::renderer::frontend::composer::transform_stack::TransformStack;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::paint_tier::PaintTier;
use crate::scene::record_store::RecordStore;
use glam::{UVec2, Vec2};
use std::num::NonZeroU32;
use std::time::Duration;

#[cfg(feature = "bench")]
pub(crate) mod bench;
mod clip_stack;
mod geometry;
mod higher_kind;
mod occlusion;
pub(crate) mod session;
// `pub(crate)` only so `bench::driver` can name `rect_grid::bench`.
pub(crate) mod rect_grid;
mod transform_stack;

/// The retained half of the CPU compose engine: buffers and stacks kept across
/// frames so steady-state rendering is alloc-free, plus the one device
/// constant a pass needs.
///
/// The pass itself is [`ComposeSession`], which holds this beside the
/// `RenderBuffer` being filled; the split is by lifetime. Nothing here takes
/// an output buffer. [`Frontend`](crate::renderer::frontend::Frontend)
/// orchestrates encode and compose.
///
/// Within a group the backend renders quads, then text, then every
/// [`PaintTier`] in `PaintTier::ALL` order (`schedule::emit_group_body`;
/// polylines ride the curve tier). That reorder is safe iff no overlapping
/// pair of draws swaps record order, enforced by forcing a
/// [`ComposeSession::flush`] when a lower-kind draw follows an overlapping
/// higher-kind one in the group, or a higher-kind draw follows an overlapping
/// draw of a later-replaying kind (a mesh after an overlapping image or
/// curve). The checks use the batch state's open text grid and
/// [`Self::higher_kinds`].
#[derive(Debug)]
pub(crate) struct Composer {
    /// The nested clips the walk has open: scissor plus rounded-mask chain per level.
    clip: ClipStack,
    /// The walk transform: live product plus the ancestors a pop restores.
    transform: TransformStack,
    polyline: PolylineScratch,
    batch: BatchState,
    /// Per-group AABBs partitioned by above-text replay tier. A lower-tier
    /// draw checks only tiers that replay after it; text and quads use the
    /// aggregate union first. Cleared per flush: the group just closed has
    /// drained, so it can no longer reorder against a still-open batch.
    higher_kinds: HigherKindRects,
    /// `*_start` cursors where the open group's per-kind slices begin;
    /// [`ComposeSession::flush`] closes each and advances it.
    cursors: GroupCursors,
    /// Per-group occlusion-prune scratch: solid-opaque occluders in the
    /// in-flight group, and the sweep dropping earlier quads they cover.
    occlusion: OcclusionPruner,
    /// Device `max_texture_dimension_2d`: the composer downsamples any
    /// composited `GpuView` whose physical rect exceeds it. Fixed per device,
    /// so it rides the ctor.
    max_texture_dim: NonZeroU32,
}

#[derive(Debug, Default)]
struct PolylineScratch {
    points: Vec<Vec2>,
    kept: Vec<u32>,
    directions: Vec<Vec2>,
}

/// Allocation-owning state for text batching. The open grid may span groups;
/// the closed grid and pending cursor reset at each group boundary.
#[derive(Debug, Default)]
struct BatchState {
    open: Option<OpenBatch>,
    open_grid: RectGrid,
    closed_grid: RectGrid,
    /// First finalized text batch not yet indexed in `closed_grid`.
    pending_batch_cursor: usize,
}

/// Per-kind slice cursors for the in-flight group: where its slice begins in
/// each output buffer. [`ComposeSession::flush`] closes the slices and
/// advances every cursor. `texts` feeds only the did-anything-emit check, so a
/// text-only group still pushes a `DrawGroup` and its batch's `last_group`
/// resolves; run spans live on
/// [`TextBatch`](crate::renderer::render_buffer::text_batch::TextBatch).
#[derive(Default, Clone, Copy, Debug)]
struct GroupCursors {
    quads: u32,
    texts: u32,
    /// One per [`PaintTier`], indexed by `PaintTier::idx`.
    higher: [u32; PaintTier::COUNT],
}

/// State carried while a text batch is mid-accumulation; pushed onto
/// `out.text_batches` as a
/// [`TextBatch`](crate::renderer::render_buffer::text_batch::TextBatch) by
/// [`ComposeSession::close_batch`].
#[derive(Clone, Copy, Debug)]
struct OpenBatch {
    /// Cursor into `out.texts` where this batch's run span begins. Recorded,
    /// not derived from the previous span end (which it always equals), so
    /// `close_batch` can assert they agree and catch a run pushed outside any
    /// batch.
    texts_start: u32,
    /// Index (into `out.groups`) of the last group whose text contributed,
    /// refreshed on every text push; tells the schedule where to emit the
    /// merged render step.
    last_group: u32,
    /// `true` once a strict run has joined: one whose ancestor clip cuts its
    /// full unclipped extent in X. The batch scissor (`open_grid.union`) must
    /// then equal that bound, so later runs join only with matching `bounds`;
    /// the text shader has no per-instance clip.
    strict: bool,
}

impl Composer {
    /// New composer capped at the device's `max_texture_dimension_2d`, all
    /// scratch empty.
    pub(crate) fn new(max_texture_dim: NonZeroU32) -> Self {
        Self {
            clip: ClipStack::default(),
            transform: TransformStack::default(),
            polyline: PolylineScratch::default(),
            batch: BatchState::default(),
            higher_kinds: HigherKindRects::default(),
            cursors: GroupCursors::default(),
            occlusion: OcclusionPruner::default(),
            max_texture_dim,
        }
    }

    /// Open a compose session over `out`: stamp the display, reset scratch and
    /// walk state, and hand back the sink. Dropping the [`ComposeSession`]
    /// closes the trailing batch and group.
    pub(crate) fn begin<'a>(
        &'a mut self,
        display: Display,
        time: Duration,
        store: &'a RecordStore,
        out: &'a mut RenderBuffer,
    ) -> ComposeSession<'a> {
        out.start_frame(display, time);

        self.reset_group_scratch(display.physical);
        self.clip.clear();
        self.transform.clear();

        ComposeSession {
            composer: self,
            store,
            out,
        }
    }

    /// Reset every piece of scratch describing composed scene output. Shared
    /// by the per-compose prologue and [`ComposeSession::discard_composed`],
    /// so a new field resets on both paths. Walk state (clip/transform stacks)
    /// is untouched: the discard path must preserve it.
    fn reset_group_scratch(&mut self, viewport_phys: UVec2) {
        self.batch.open_grid.start_frame(viewport_phys);
        self.batch.closed_grid.start_frame(viewport_phys);
        self.batch.pending_batch_cursor = 0;
        self.higher_kinds.start_frame(viewport_phys);
        self.cursors = GroupCursors::default();
        self.batch.open = None;
        self.occlusion.start_frame(viewport_phys);
    }
}

#[cfg(any(test, feature = "bench"))]
pub(crate) mod internals {
    //! Replay driver for the composer tests and the compose bench.

    use crate::internals::paint_capture::PaintCapture;
    use crate::renderer::frontend::composer::session::ComposeSession;

    impl ComposeSession<'_> {
        /// Replay a recorded paint stream into this session, closing it, so
        /// tests and benches can drive the composer from a stream captured once.
        pub(crate) fn replay_from(mut self, recorded: &PaintCapture) {
            recorded.replay(&mut self);
        }
    }
}

#[cfg(test)]
mod tests;
