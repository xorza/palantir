//! Turning a frame of paint calls into the buffer the backend draws.
//!
//! [`Composer`] owns the output and the scratch it is built in; a
//! [`ComposeSession`] is one frame passing through it, and [`geometry`] is
//! the arithmetic both are cut with.

use crate::display::Display;
use crate::primitives::span::Span;
use crate::primitives::transform::TranslateScale;
use crate::primitives::urect::URect;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::batch::{DrawGroup, GroupBatch, PaintTier, TextBatch};
use crate::scene::record_store::RecordPayloads;
use glam::{UVec2, Vec2};
use std::num::NonZeroU32;

#[cfg(feature = "bench")]
pub(crate) mod bench;
mod geometry;
mod higher_kind;
mod occlusion;
pub(crate) mod session;
// `pub(crate)` only so the `text_grid` benchmark can reach the gated
// `internals` harness; every item inside stays `pub(super)`.
pub(crate) mod text_grid;

use crate::renderer::frontend::composer::geometry::chains_equal;
use crate::renderer::frontend::composer::session::ComposeSession;
use higher_kind::HigherKindRects;
use occlusion::OcclusionPruner;
use text_grid::TextRectGrid;

/// CPU-only compose engine: turns the encoder's paint calls into a `RenderBuffer`
/// (physical-px quads + text runs + scissor groups), one [`ComposeSession`] per frame. Owns its output buffer
/// + compose-time scratch stacks so steady-state rendering is alloc-free.
///
/// Composer doesn't know about `Tree` or `encode` — it's pure algorithm +
/// scratch + output. [`Frontend`](crate::renderer::frontend::Frontend) orchestrates
/// encode + compose.
///
/// Render order *within* a group is fixed by the backend:
/// **quads → text → meshes → images → curves**
/// (`schedule::emit_group_body`; polylines ride the curve tier as
/// segment + join-chrome instances). That
/// reorder is safe iff no overlapping pair of draws swaps its record
/// order — two rules, both enforced by forcing a [`Self::flush`]:
/// a *lower*-kind draw must not follow an overlapping higher-kind draw
/// in the same group (it would replay under it), and a *higher*-kind
/// draw must not follow an overlapping higher-kind draw of a
/// later-replaying kind (e.g. a mesh recorded after an overlapping
/// image or curve). The checks use
/// the batch state's open text grid (per-batch text AABBs, spatially indexed)
/// and [`higher_kinds`](Self::higher_kinds) (per-group, per-tier
/// AABBs of mesh/image/curve draws).
#[derive(Debug)]
pub(crate) struct Composer {
    /// Compose-time scratch — bounded by tree depth (typically <8).
    /// Pairs the resolved scissor with its rounded-mask chain; both
    /// ride together so a `PopClip` restores them as a unit.
    clip_stack: Vec<ClipFrame>,
    transform_stack: Vec<TranslateScale>,
    polyline: PolylineScratch,
    batch: BatchState,
    /// Per-group AABBs partitioned by above-text replay tier. A later
    /// lower-tier draw checks only tiers that replay after it, while
    /// text and quads use the aggregate union before scanning any set.
    /// Cleared per flush — independent of batch state since every
    /// higher-kind draw also closes the batch.
    higher_kinds: HigherKindRects,
    /// In-flight group clip state: the active scissor + rounded-mask
    /// chain stamped onto the group at [`Self::flush`]. Changed only
    /// through [`Self::set_clip`], which flushes when either differs
    /// (chains compare by value, so a pop/re-push of an identical
    /// rounded clip stays a no-op).
    current_scissor: Option<URect>,
    current_chain: Span,
    /// `*_start` cursors marking where the open group's per-kind slices
    /// begin in `out`. [`Self::flush`] closes each slice and advances
    /// the matching cursor.
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

#[derive(Clone, Copy, Debug)]
struct ClipFrame {
    scissor: URect,
    /// Outer→inner chain of rounded masks active for this frame's
    /// subtree — a span into `RenderBuffer.rounded_clips`. A rounded
    /// push extends the parent chain with its own mask; a rect push
    /// inherits it verbatim. Empty = no rounded ancestor.
    chain: Span,
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
/// where the open group's slice begins in the matching `out` buffer;
/// [`Composer::flush`] closes the slices and advances every cursor to
/// the buffer's current length. Bundled so the flush-boundary contract
/// is one value instead of five parallel fields. `texts` feeds only the
/// did-anything-emit check — a text-only group must still push a
/// `DrawGroup` so its batch's `last_group` index resolves; the run
/// spans themselves live on [`TextBatch`].
#[derive(Default, Clone, Copy, Debug)]
struct GroupCursors {
    quads: u32,
    texts: u32,
    /// One per [`PaintTier`], indexed by `PaintTier::idx` — the four
    /// higher-kind columns are walked, never named individually.
    higher: [u32; PaintTier::COUNT],
}

/// State carried while a text batch is mid-accumulation. Pushed onto
/// `out.text_batches` as a [`TextBatch`] when [`Composer::close_batch`]
/// finalizes it.
#[derive(Clone, Copy, Debug)]
struct OpenBatch {
    /// Cursor into `out.texts` where this batch's run span begins.
    /// Combined with `out.texts.len()` at close-time to compute the
    /// finalized [`Span`].
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
            clip_stack: Vec::new(),
            transform_stack: Vec::new(),
            polyline: PolylineScratch::default(),
            batch: BatchState::default(),
            higher_kinds: HigherKindRects::default(),
            current_scissor: None,
            current_chain: Span::default(),
            cursors: GroupCursors::default(),
            occlusion: OcclusionPruner::default(),
            max_texture_dim: NonZeroU32::new(max_texture_dim)
                .expect("composer texture dimension limit must be positive"),
        }
    }

    /// Close the in-flight group: if anything was emitted into it,
    /// push a `DrawGroup` covering the open slice; either way advance
    /// the per-kind cursors and clear the overlap scratches. Scissor
    /// + rounded clip are preserved for the next group.
    fn flush(&mut self, out: &mut RenderBuffer) {
        self.occlusion.prune(out, self.cursors.quads);
        let q_end = out.quads.len() as u32;
        let t_end = out.texts.len() as u32;
        let higher_end = PaintTier::ALL.map(|tier| out.draws_len(tier));
        if q_end > self.cursors.quads
            || t_end > self.cursors.texts
            || PaintTier::ALL
                .iter()
                .any(|&t| higher_end[t.idx()] > self.cursors.higher[t.idx()])
        {
            // Push the higher-kind batches BEFORE the group itself so
            // their `last_group` matches the in-flight group's
            // eventual index (= current `out.groups.len()`).
            let last_group = out.groups.len() as u32;
            for tier in PaintTier::ALL {
                let start = self.cursors.higher[tier.idx()];
                let end = higher_end[tier.idx()];
                if end > start {
                    out.batches_mut(tier).push(GroupBatch {
                        items: (start..end).into(),
                        last_group,
                    });
                }
            }
            out.groups.push(DrawGroup {
                scissor: self.current_scissor,
                rounded_clips: self.current_chain,
                quads: (self.cursors.quads..q_end).into(),
            });
        }
        self.cursors = GroupCursors {
            quads: q_end,
            texts: t_end,
            higher: higher_end,
        };
        self.higher_kinds.clear();
        self.occlusion.clear();
        // Closed-batch text is group-scoped: once we cross a group
        // boundary, any batch closed *in* this group has rendered (it
        // drains at its `last_group`), so its rects no longer gate quads.
        // The open-batch grid is NOT cleared here — it spans groups with
        // its (still-open) batch.
        self.batch.closed_grid.clear();
        self.batch.pending_batch_cursor = out.text_batches.len();
    }

    /// Finalize the open text batch (if any): push a [`TextBatch`]
    /// entry covering `batch_texts_start..out.texts.len()`. No-op when no
    /// batch is active. Called at batch-split events — rounded-clip
    /// change, a higher-kind append, or a strict-bounds mismatch. The
    /// finalized output remains pending for the group-scoped closed
    /// check, so a later quad still flushes for already-closed text that
    /// shares this group. The grid fill is deferred to [`Self::closed_hit`].
    fn close_batch(&mut self, out: &mut RenderBuffer) {
        let Some(b) = self.batch.open.take() else {
            return;
        };
        let texts_end = out.texts.len() as u32;
        let scissor = self.batch.open_grid.union;
        self.batch.open_grid.clear();
        // Invariants the schedule cursor relies on: batches are pushed
        // in walk order so `last_group` is monotonically non-decreasing
        // (multiple batches can anchor to the same group when a mesh
        // splits mid-group), and their `texts` spans concatenate
        // without gaps in `out.texts`.
        debug_assert!(
            out.text_batches
                .last()
                .is_none_or(|prev| prev.last_group <= b.last_group),
        );
        debug_assert!(
            out.text_batches
                .last()
                .is_none_or(|prev| prev.texts.start + prev.texts.len == b.texts_start),
        );
        out.text_batches.push(TextBatch {
            texts: (b.texts_start..texts_end).into(),
            last_group: b.last_group,
            // `scissor` is already in physical pixels and clamped to
            // every contributing run's clip-stack-narrowed bounds, so it
            // is the GPU scissor for this batch. It has to be: the text
            // backend implements no per-run shader clipping, so a
            // scissor any wider than this would let a clipped run's
            // glyphs paint past their intended bound.
            scissor,
            // Every close site runs before `current_chain` can change
            // (set_clip closes ahead of the update), so this is the
            // chain all the batch's runs were recorded under.
            rounded_clips: self.current_chain,
        });
    }

    /// Return a mutable handle to the open batch, opening a fresh one
    /// when none exists. Idempotent within a batch — repeated calls
    /// reuse the same `OpenBatch` and only refresh `last_group` to
    /// the in-flight group's eventual index.
    fn open_batch(&mut self, out: &RenderBuffer) -> &mut OpenBatch {
        let b = self.batch.open.get_or_insert(OpenBatch {
            texts_start: out.texts.len() as u32,
            last_group: 0,
            strict: false,
        });
        b.last_group = out.groups.len() as u32;
        b
    }

    /// `true` when `bounds` has no viewport area or doesn't intersect
    /// the active scissor — the caller should skip emission. Identical
    /// reject shape at every shape-draw site; centralising it keeps each
    /// handler from growing its own variant.
    fn cull_bounds(&self, bounds: URect) -> bool {
        bounds.is_paint_empty() || self.current_scissor.is_some_and(|s| !bounds.intersects(s))
    }

    /// Cull a higher-kind (mesh / image / curve) draw against the active
    /// clip and, if it survives, close any open text batch. Higher-kind
    /// geometry paints above text under the backend's kind reorder, and a
    /// batch renders at the END of its last group — past this draw if left
    /// open — so the batch must close here for its text to emit first. Done
    /// only after the cull: a culled draw must not split the batch. Also
    /// flushes the group when the draw cross-kind-conflicts with an earlier
    /// higher-kind draw (see [`HigherKindRects::conflicts`]), and then
    /// records the draw's own rect for the group's overlap tracking (after
    /// the flush, so it isn't wiped with the previous group's rects).
    /// Returns `false` when culled — the caller should `continue`.
    ///
    /// Polyline calls this only after its kept-point walk proves the
    /// stroke emits geometry (an all-coincident polyline must not split
    /// the batch), gated behind an early [`Self::cull_bounds`].
    fn enter_higher_kind(
        &mut self,
        tier: PaintTier,
        bounds: URect,
        out: &mut RenderBuffer,
    ) -> bool {
        if self.cull_bounds(bounds) {
            return false;
        }
        self.close_batch(out);
        if self.higher_kinds.conflicts(tier, bounds) {
            self.flush(out);
        }
        self.higher_kinds.push(tier, bounds);
        true
    }

    /// Conservative overlap of `rect` against every recorded higher-kind
    /// draw, kind-blind: any non-empty intersection counts. False
    /// positives are correctness-safe (extra flush, costs a drawcall);
    /// false negatives would reorder paint and corrupt the frame.
    fn any_higher_kind_overlap(&self, rect: URect) -> bool {
        self.higher_kinds.any_overlap(rect)
    }

    /// Force a flush / batch-close if a quad-tier draw at `overlap`
    /// overlaps something in the group that would be reordered above it.
    /// Quad is the lowest paint kind, so any higher-kind draw it overlaps
    /// would paint *under* it after the backend's intra-group reorder —
    /// flush to keep record order. Text overlap is checked against both
    /// the open batch's grid (which may span groups) and
    /// batches already closed in this group ([`Self::closed_hit`]);
    /// an open-batch hit additionally closes the batch so its text can't
    /// coalesce forward and re-cover this quad. The open check goes
    /// straight to the tiled grid — `any_overlap` pre-rejects on its
    /// internal union AABB, so no caller-side pre-reject is needed.
    fn quad_forces_flush(&mut self, overlap: URect, out: &mut RenderBuffer) {
        // Text painted in (or scheduled after) this group sits in two
        // places: the open batch (`open_grid`, spans groups with its
        // batch) and batches already closed within this group
        // (`closed_grid`). A quad overlapping either would be painted
        // *under* that text by the backend's quads→text order, so flush so
        // the text renders first.
        //
        // An open-batch hit additionally *closes* the batch: leaving it
        // open would let the overlapping run coalesce forward and schedule
        // at a later `last_group`, re-covering this quad. A closed-grid
        // hit needs no close — that text's batch is already finalized at
        // this group; flushing alone puts the quad in the next group.
        if self.batch.open_grid.any_overlap(overlap) {
            self.close_batch(out);
            self.flush(out);
        } else if self.closed_hit(overlap, out) || self.any_higher_kind_overlap(overlap) {
            self.flush(out);
        }
    }

    /// `true` if `q` overlaps text of a batch closed within the
    /// in-flight group. Finalized batches remain pending in
    /// `out.text_batches`; the first query whose `q` hits a pending
    /// batch scissor drains every pending batch into the closed grid.
    /// Later queries use the grid, and groups nothing probes near
    /// closed text never pay the per-rect fill.
    fn closed_hit(&mut self, q: URect, out: &RenderBuffer) -> bool {
        let pending = &out.text_batches[self.batch.pending_batch_cursor..];
        if pending.iter().any(|batch| batch.scissor.intersects(q)) {
            for batch in pending {
                for ti in batch.texts.range() {
                    self.batch.closed_grid.push(out.texts[ti].bounds);
                }
            }
            self.batch.pending_batch_cursor = out.text_batches.len();
        }
        self.batch.closed_grid.any_overlap(q)
    }

    /// Switch to a new clip (scissor + rounded-mask chain), flushing
    /// the in-flight group only if anything actually differs. Chains
    /// compare by value, so a same-clip Push/Pop is a no-op and
    /// accumulated overlap state persists through redundant clip
    /// transitions.
    fn set_clip(&mut self, scissor: Option<URect>, chain: Span, out: &mut RenderBuffer) {
        let chain_changed = !chains_equal(out, chain, self.current_chain);
        if chain_changed {
            // The stencil mask stack is tied to the active chain;
            // batched text under the wrong masks would either over- or
            // under-clip. Close before the group transition (while
            // `current_chain` still names the batch's chain).
            self.close_batch(out);
        }
        if scissor != self.current_scissor || chain_changed {
            self.flush(out);
            self.current_scissor = scissor;
            self.current_chain = chain;
        }
    }

    /// Open a compose session over `out`: stamp the frame's display, reset
    /// scratch + walk state, and hand back the sink paint streams into.
    /// Dropping the [`ComposeSession`] closes the trailing batch and group.
    pub(crate) fn begin<'a>(
        &'a mut self,
        display: Display,
        payloads: &'a RecordPayloads,
        out: &'a mut RenderBuffer,
    ) -> ComposeSession<'a> {
        out.start_frame(display);

        self.reset_group_scratch(display.physical);
        self.clip_stack.clear();
        self.transform_stack.clear();
        self.current_scissor = None;
        self.current_chain = Span::default();

        ComposeSession {
            composer: self,
            payloads,
            out,
            display,
            current_transform: TranslateScale::IDENTITY,
        }
    }

    /// Clear-fold discard: a fullscreen opaque cover proved everything
    /// composed so far invisible — drop the scene output and every piece of
    /// scratch that describes it. The *walk* state survives: `clip_stack` /
    /// `current_scissor` / `current_chain` are empty by the fold's
    /// precondition, and `transform_stack` + the caller's running transform
    /// stay untouched (the cover may sit under an active transform whose
    /// pops are still ahead in the stream).
    fn discard_composed(&mut self, out: &mut RenderBuffer) {
        out.discard_scene();
        self.reset_group_scratch(out.viewport_phys);
    }

    /// Reset every piece of scratch that describes composed *scene*
    /// output — group cursors, batch state, overlap tracking. Shared
    /// by the per-compose prologue and the clear-fold
    /// [`Self::discard_composed`], so a new scratch field added here
    /// resets on both paths. Walk state (clip/transform stacks, the
    /// active scissor + chain) is deliberately not touched — the
    /// discard path must preserve it.
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
pub(crate) mod internals {
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
