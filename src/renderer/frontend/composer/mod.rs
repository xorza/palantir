//! Turning a frame of paint calls into the buffer the backend draws.
//!
//! [`Composer`] owns the scratch a compose pass is built in; a
//! [`ComposeSession`] is one pass, holding that scratch together with the
//! buffer it fills, and [`geometry`] is the arithmetic both are cut with.

use crate::display::Display;
use crate::renderer::frontend::composer::clip_stack::ClipStack;
use crate::renderer::frontend::composer::transform_stack::TransformStack;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::paint_tier::PaintTier;
use crate::scene::record_store::record_payloads::RecordPayloads;
use glam::{UVec2, Vec2};
use std::num::NonZeroU32;

#[cfg(feature = "bench")]
pub(crate) mod bench;
mod clip_stack;
mod geometry;
mod higher_kind;
mod occlusion;
pub(crate) mod session;
// `pub(crate)` only so `bench::driver` — the crate-root facade the
// external criterion target calls through — can name `text_grid::bench`.
pub(crate) mod text_grid;
mod transform_stack;

use crate::renderer::frontend::composer::higher_kind::HigherKindRects;
use crate::renderer::frontend::composer::occlusion::OcclusionPruner;
use crate::renderer::frontend::composer::session::ComposeSession;
use crate::renderer::frontend::composer::text_grid::TextRectGrid;
use std::time::Duration;

/// The retained half of the CPU compose engine: every buffer and stack a
/// pass is built in, kept across frames so steady-state rendering is
/// alloc-free, plus the one device constant a pass needs.
///
/// **Scratch, not algorithm.** The pass itself is
/// [`ComposeSession`] — it holds this alongside the `RenderBuffer` being
/// filled, and every step that reads or writes that buffer is a method on
/// it. The split is by *lifetime*: what survives a frame lives here, what
/// belongs to one frame lives there. Nothing here takes an output buffer,
/// which is what keeps that line honest.
///
/// Composer doesn't know about `Tree` or `encode` —
/// [`Frontend`](crate::renderer::frontend::Frontend) orchestrates
/// encode + compose.
///
/// Render order *within* a group is fixed by the backend:
/// **quads → text → meshes → images → curves**
/// (`schedule::emit_group_body`; polylines ride the curve tier as
/// segment + join-chrome instances). That
/// reorder is safe iff no overlapping pair of draws swaps its record
/// order — two rules, both enforced by forcing a
/// [`ComposeSession::flush`]:
/// a *lower*-kind draw must not follow an overlapping higher-kind draw
/// in the same group (it would replay under it), and a *higher*-kind
/// draw must not follow an overlapping higher-kind draw of a
/// later-replaying kind (e.g. a mesh recorded after an overlapping
/// image or curve). The checks use
/// the batch state's open text grid (per-batch text AABBs, spatially indexed)
/// and [`Self::higher_kinds`] (per-group, per-tier
/// AABBs of mesh/image/curve draws).
#[derive(Debug)]
pub(crate) struct Composer {
    /// The nested clips the walk has open — resolved scissor plus
    /// rounded-mask chain per level.
    clip: ClipStack,
    /// The walk transform: live product plus the ancestors a pop
    /// restores it from.
    transform: TransformStack,
    polyline: PolylineScratch,
    batch: BatchState,
    /// Per-group AABBs partitioned by above-text replay tier. A later
    /// lower-tier draw checks only tiers that replay after it, while
    /// text and quads use the aggregate union before scanning any set.
    /// Cleared per flush — independent of batch state since every
    /// higher-kind draw also closes the batch.
    higher_kinds: HigherKindRects,
    /// `*_start` cursors marking where the open group's per-kind slices
    /// begin in the output. [`ComposeSession::flush`] closes each slice
    /// and advances the matching cursor.
    cursors: GroupCursors,
    /// Per-group occlusion-prune scratch: the solid-opaque occluders
    /// pushed into the in-flight group and the sweep that drops earlier
    /// quads they fully cover. See [`OcclusionPruner`].
    occlusion: OcclusionPruner,
    /// Device `max_texture_dimension_2d`, the cap on a `GpuView` off-screen
    /// target's size — the composer uniformly downsamples each composited
    /// `GpuView` whose physical rect exceeds it. Fixed for the device's
    /// lifetime, so it rides the ctor, not every compose.
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
    open_grid: TextRectGrid,
    closed_grid: TextRectGrid,
    /// First finalized text batch not yet indexed in `closed_grid`.
    pending_batch_cursor: usize,
}

/// Per-kind slice cursors for the in-flight group. Each field marks
/// where the open group's slice begins in the matching output buffer;
/// [`ComposeSession::flush`] closes the slices and advances every cursor
/// to the buffer's current length. Bundled so the flush-boundary contract
/// is one value instead of a loose cursor field per output buffer. `texts` feeds only the
/// did-anything-emit check — a text-only group must still push a
/// `DrawGroup` so its batch's `last_group` index resolves; the run
/// spans themselves live on [`TextBatch`](crate::renderer::render_buffer::text_batch::TextBatch).
#[derive(Default, Clone, Copy, Debug)]
struct GroupCursors {
    quads: u32,
    texts: u32,
    /// One per [`PaintTier`], indexed by `PaintTier::idx` — the four
    /// higher-kind columns are walked, never named individually.
    higher: [u32; PaintTier::COUNT],
}

/// State carried while a text batch is mid-accumulation. Pushed onto
/// `out.text_batches` as a
/// [`TextBatch`](crate::renderer::render_buffer::text_batch::TextBatch)
/// when [`ComposeSession::close_batch`] finalizes it.
#[derive(Clone, Copy, Debug)]
struct OpenBatch {
    /// Cursor into `out.texts` where this batch's run span begins.
    /// Combined with `out.texts.len()` at close-time to compute the
    /// finalized [`Span`](crate::primitives::span::Span).
    ///
    /// Recorded rather than derived from the previous batch's span end,
    /// which it always equals. Deriving it would make the two agree by
    /// construction; recording it lets `close_batch` assert they do, and
    /// so catch a run pushed to `out.texts` outside any batch — which
    /// derivation would silently absorb into the next batch's span.
    texts_start: u32,
    /// Index (into `out.groups`) of the last group whose text
    /// contributed to this batch. Refreshed on every text push (the
    /// in-flight group's eventual index is `out.groups.len()`).
    /// Tells the schedule where to emit the merged render step.
    last_group: u32,
    /// `true` once a "strict" run has joined this batch — one whose
    /// ancestor clip cuts its full unclipped extent in X. The batch's
    /// GPU scissor (= `open_grid.union`) must then stay equal to that
    /// strict bound; subsequent runs can only join if their `bounds`
    /// match exactly. Otherwise the merged scissor would let the
    /// strict run's glyphs paint past their intended clip (the text
    /// shader has no per-instance clip).
    strict: bool,
}

impl Composer {
    /// New composer capped at the device's `max_texture_dimension_2d` (the
    /// `GpuView` target-size ceiling). All scratch starts empty.
    pub(crate) fn new(max_texture_dim: u32) -> Self {
        Self {
            clip: ClipStack::default(),
            transform: TransformStack::default(),
            polyline: PolylineScratch::default(),
            batch: BatchState::default(),
            higher_kinds: HigherKindRects::default(),
            cursors: GroupCursors::default(),
            occlusion: OcclusionPruner::default(),
            max_texture_dim: NonZeroU32::new(max_texture_dim)
                .expect("composer texture dimension limit must be positive"),
        }
    }

    /// Open a compose session over `out`: stamp the frame's display, reset
    /// scratch + walk state, and hand back the sink paint streams into.
    /// Dropping the [`ComposeSession`] closes the trailing batch and group.
    pub(crate) fn begin<'a>(
        &'a mut self,
        display: Display,
        time: Duration,
        payloads: &'a RecordPayloads,
        out: &'a mut RenderBuffer,
    ) -> ComposeSession<'a> {
        out.start_frame(display, time);

        self.reset_group_scratch(display.physical);
        self.clip.clear();
        self.transform.clear();

        ComposeSession {
            composer: self,
            payloads,
            out,
        }
    }

    /// Reset every piece of scratch that describes composed *scene*
    /// output — group cursors, batch state, overlap tracking. Shared
    /// by the per-compose prologue and the clear-fold
    /// [`ComposeSession::discard_composed`], so a new scratch field added
    /// here resets on both paths. Walk state (clip/transform stacks) is
    /// deliberately not touched — the discard path must preserve it.
    fn reset_group_scratch(&mut self, viewport_phys: UVec2) {
        self.batch.open_grid.start_frame(viewport_phys);
        self.batch.closed_grid.start_frame(viewport_phys);
        self.batch.pending_batch_cursor = 0;
        self.higher_kinds.clear();
        self.cursors = GroupCursors::default();
        self.batch.open = None;
        self.occlusion.clear();
    }
}

#[cfg(any(test, feature = "bench"))]
pub(crate) mod test_support {
    //! Replay driver for the composer tests and the compose bench.

    use crate::renderer::frontend::capture::PaintCapture;
    use crate::renderer::frontend::composer::session::ComposeSession;

    impl ComposeSession<'_> {
        /// Replay a recorded paint stream into this session, closing it
        /// on return. Lets tests and benches drive the composer from a
        /// stream captured once, outside whatever they are measuring or
        /// asserting on.
        pub(crate) fn replay_from(mut self, recorded: &PaintCapture) {
            recorded.replay(&mut self);
        }
    }
}

#[cfg(test)]
mod tests;
