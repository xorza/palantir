//! Paint order: which draws may reorder within a group, and where a group or a text batch must
//! close so none that overlap does.

use crate::common::counters::TestOnly;
use crate::common::span::Span;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::urect::URect;
use crate::renderer::backdrop_captures::BackdropTicket;
use crate::renderer::frontend::composer::backdrop_break::BackdropBreak;
use crate::renderer::frontend::composer::clip_stack::{ClipFrame, ClipStack};
use crate::renderer::frontend::composer::higher_kind_rects::HigherKindRects;
use crate::renderer::frontend::composer::occlusion_pruner::OcclusionPruner;
use crate::renderer::frontend::composer::quad_runs::QuadRuns;
use crate::renderer::frontend::composer::rect_grid::RectGrid;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::backdrop_row::BackdropSource;
use crate::renderer::render_buffer::draw_group::DrawGroup;
use crate::renderer::render_buffer::group_batch::GroupBatch;
use crate::renderer::render_buffer::paint_tier::PaintTier;
use crate::renderer::render_buffer::text_batch::TextBatch;
use crate::renderer::render_buffer::text_draw_row::TextDrawRow;
use glam::UVec2;

/// Where a draw replays within its group: the backend draws a group's quads, then its text, then
/// each [`PaintTier`] in `PaintTier::ALL` order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Lane {
    Quad,
    Text,
    Tier(PaintTier),
}

/// Why a group closed: the clip in force changed its scissor, a draw would have swapped paint order
/// with one it overlaps, a backdrop's barrier, or the session's end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GroupClose {
    Clip,
    PaintOrder,
    Barrier,
    End,
}

impl GroupClose {
    const COUNT: usize = 4;
}

/// A frame's closed groups by cause, and the draws each cause's closes had after them.
#[derive(Clone, Copy, Debug, Default)]
struct CloseTally {
    /// The draws ordered so far in the frame.
    draws: u32,
    /// The groups closed, by [`GroupClose`].
    groups: [u32; GroupClose::COUNT],
    /// The draws ordered before each close, summed by its cause: a cause's draws after its closes
    /// are its groups times the frame's draws, less these.
    draws_before: [u32; GroupClose::COUNT],
}

/// The retained state of paint order: the open text batch, the higher-kind rects, where the open
/// group's slices begin, the occluders the group prunes with, and the backdrop break.
///
/// Within a group the backend replays by [`Lane`], not in record order. That is safe iff no
/// overlapping pair of draws swaps record order, which [`Ordering::order`] enforces by closing the
/// group, or the text batch, where a draw follows an overlapping one of a lane that replays after
/// it.
#[derive(Debug, Default)]
pub(super) struct PaintOrder {
    batch: BatchState,
    /// Per-group AABBs partitioned by above-text replay tier. A lower-tier draw checks only tiers
    /// that replay after it; text and quads use the aggregate union first. Cleared per flush: the
    /// group just closed has drained, so it can no longer reorder against a still-open batch.
    higher_kinds: HigherKindRects,
    /// Where the open group's per-kind slices begin; [`Ordering::flush`] closes each and advances
    /// it.
    cursors: GroupCursors,
    /// Solid-opaque occluders in the open group, and the sweep dropping earlier quads they cover.
    occlusion: OcclusionPruner,
    /// The reorder that joins the closing group's quads into one run per draw where it can.
    runs: QuadRuns,
    /// The last break the main pass takes for a backdrop, and what is painted after it.
    backdrop_break: BackdropBreak,
    /// Why this frame's groups closed, which the batching census reads.
    closes: TestOnly<CloseTally>,
}

/// Allocation-owning state for text batching. The open grid may span groups; the closed grid and
/// pending cursor reset at each group boundary. `open` and `open_grid` change together: the grid
/// holds runs only while a batch is open.
#[derive(Debug, Default)]
struct BatchState {
    open: Option<OpenBatch>,
    open_grid: RectGrid,
    closed_grid: RectGrid,
    /// First finalized text batch not yet indexed in `closed_grid`.
    pending_batch_cursor: usize,
}

/// A text batch mid-accumulation, pushed onto `out.text_batches` as a [`TextBatch`] by
/// [`Ordering::close_batch`].
#[derive(Clone, Copy, Debug)]
struct OpenBatch {
    /// Cursor into `out.texts` where this batch's run span begins. Recorded, not derived from the
    /// previous span end (which it always equals), so the close can assert they agree and catch a
    /// run pushed outside any batch.
    texts_start: u32,
    /// Index (into `out.groups`) of the last group whose text contributed, refreshed on every text
    /// push; tells the schedule where to emit the merged render step.
    last_group: u32,
    /// [`TextClip::Cut`] once a cut run has joined. The batch scissor (`open_grid.union()`) must then
    /// equal that run's bounds, so later runs join only with matching `bounds`; a turned run's
    /// glyphs carry no clip rect of their own.
    clip: TextClip,
}

/// Whether an ancestor clip cuts a turned text run's unclipped extent, which its batch's scissor
/// alone clips; an unturned run clips its own glyphs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TextClip {
    Whole,
    Cut,
}

/// A text batch [`BatchState::close`] took, and the scissor its runs share.
#[derive(Clone, Copy, Debug)]
struct ClosedBatch {
    batch: OpenBatch,
    scissor: URect,
}

/// Per-kind slice cursors for the open group: where its slice begins in each output buffer.
/// `texts` feeds only the did-anything-emit check, so a text-only group still pushes a `DrawGroup`
/// and its batch's `last_group` resolves; run spans live on [`TextBatch`].
#[derive(Default, Clone, Copy, Debug)]
struct GroupCursors {
    quads: u32,
    texts: u32,
    /// One per [`PaintTier`], indexed by `PaintTier::idx`.
    higher: [u32; PaintTier::COUNT],
}

impl BatchState {
    fn start_frame(&mut self, viewport: UVec2) {
        self.open = None;
        self.open_grid.start_frame(viewport);
        self.closed_grid.start_frame(viewport);
        self.pending_batch_cursor = 0;
    }

    /// Take the open batch and empty its grid, the two together.
    fn close(&mut self) -> Option<ClosedBatch> {
        let batch = self.open.take()?;
        let scissor = self.open_grid.union();
        self.open_grid.clear();
        Some(ClosedBatch { batch, scissor })
    }
}

impl PaintOrder {
    /// Drop every piece of state describing composed output, before the first draw of new scene
    /// output: at compose start and after a clear-fold discard.
    pub(super) fn start_frame(&mut self, viewport: UVec2) {
        self.batch.start_frame(viewport);
        self.higher_kinds.start_frame(viewport);
        self.cursors = GroupCursors::default();
        self.occlusion.start_frame(viewport);
        self.backdrop_break.open(0);
        self.closes.reset();
    }
}

/// Paint order over the buffer one compose session fills, under the clip in force.
#[derive(Debug)]
pub(super) struct Ordering<'s> {
    pub(super) order: &'s mut PaintOrder,
    pub(super) clip: &'s ClipStack,
    pub(super) out: &'s mut RenderBuffer,
}

impl Ordering<'_> {
    /// [`Self::cull`] `bounds`, then [`Self::order`] a draw of `lane` there: the pixels it paints,
    /// or `None` when it paints none, which then neither splits a batch nor registers a rect.
    #[inline(always)]
    pub(super) fn admit(&mut self, lane: Lane, bounds: URect) -> Option<URect> {
        let visible = self.cull(bounds)?;
        self.order(lane, visible);
        Some(visible)
    }

    /// The part of `bounds` the clip in force leaves, `None` when it is empty.
    #[inline(always)]
    pub(super) fn cull(&self, bounds: URect) -> Option<URect> {
        let visible = self.clip.clamped(bounds);
        (!visible.is_paint_empty()).then_some(visible)
    }

    /// Close what a draw of `lane` at `visible` would reorder against, register it, and count it
    /// as painted since the backdrop break.
    ///
    /// A quad closes the group where it overlaps anything in it that replays above it: text of the
    /// open batch that renders at the group's end, text of a batch closed in the group, a
    /// higher-kind draw. Text closes it where it overlaps a higher-kind draw. A higher-kind draw
    /// closes the open text batch where it covers text already in it, as a batch renders at the
    /// end of its last group and text recorded before this draw would paint over it; and the
    /// group where it overlaps a tier that replays after it. A close on every draw would cost one
    /// text batch per draw (one per toolbar button); either half alone is unsound.
    // Always inlined: each caller's `lane` is a constant, so one arm remains and `visible` stays
    // in registers. As a call it took `visible` through memory, and `same_tier_image` ran 5 %
    // slower.
    #[inline(always)]
    pub(super) fn order(&mut self, lane: Lane, visible: URect) {
        match lane {
            Lane::Quad => self.quad_forces_flush(visible),
            Lane::Text => {
                if self.order.higher_kinds.any_overlap(visible) {
                    self.flush(GroupClose::PaintOrder);
                }
            }
            Lane::Tier(tier) => {
                if self.order.batch.open_grid.any_overlap(visible) {
                    self.close_batch();
                }
                if self.order.higher_kinds.conflicts(tier, visible) {
                    self.flush(GroupClose::PaintOrder);
                }
                self.order.higher_kinds.push(tier, visible);
            }
        }
        self.order.backdrop_break.paint(visible);
        self.order.closes.edit(|tally| tally.draws += 1);
    }

    /// Where a backdrop of `ticket` reads: its cached result where it holds one, else a
    /// capture at a break of the main pass. A capturing backdrop whose footprint misses everything
    /// painted since the last break shares it, and shows the clear when that is `0`, before any;
    /// else the text batch and the group close at a new barrier, so nothing before it coalesces,
    /// reorders or prunes against what follows: the main pass can break there and read the
    /// pixels the groups before it painted.
    pub(super) fn backdrop_source(&mut self, ticket: BackdropTicket) -> BackdropSource {
        if !ticket.captures {
            return BackdropSource::Held;
        }
        let barrier = self
            .order
            .backdrop_break
            .shared(ticket.capture.footprint)
            .unwrap_or_else(|| {
                self.close_batch();
                self.flush(GroupClose::Barrier);
                let barrier = self.out.groups.len() as u32;
                self.order.backdrop_break.open(barrier);
                barrier
            });
        if barrier == 0 {
            BackdropSource::Clear
        } else {
            BackdropSource::Capture { barrier }
        }
    }

    /// Push the text run `row` into the open batch, opening one where none is or where its scissor
    /// cannot hold the run: a run `clip` cuts batches only with peers of identical bounds, as the
    /// text shader has no per-instance clip. The batch spans groups, the one draw whose order is
    /// not group-scoped.
    pub(super) fn push_text(&mut self, row: TextDrawRow, clip: TextClip) {
        let bounds = row.bounds;
        if let Some(b) = self.order.batch.open.as_ref()
            && (b.clip == TextClip::Cut || clip == TextClip::Cut)
            && self.order.batch.open_grid.union() != bounds
        {
            self.close_batch();
        }
        // Opened before the push, so `texts_start` takes this run's index.
        let b = self.open_batch();
        if clip == TextClip::Cut {
            b.clip = TextClip::Cut;
        }
        self.out.texts.push(row);
        self.order.batch.open_grid.push(bounds);
    }

    /// Note the quad just pushed as an occluder covering `cover`, for the group's prune.
    pub(super) fn record_opaque(&mut self, cover: Rect) {
        let idx = self.out.quads.len() as u32 - 1 - self.order.cursors.quads;
        self.order.occlusion.record_opaque(idx, cover);
    }

    /// Close the group the clip in force owns, if `next` has another scissor. A rounded clip's
    /// chain rides each draw, so it closes nothing. Runs before the stack moves, since
    /// [`Self::flush`] stamps the closing group with the stack top.
    pub(super) fn break_for_clip(&mut self, next: Option<ClipFrame>) {
        if next.map(|frame| frame.scissor) != self.clip.scissor() {
            self.flush(GroupClose::Clip);
        }
    }

    /// Close the open group for `cause`: push a `DrawGroup` if anything was emitted, advance the
    /// per-kind cursors and clear the overlap scratches. Scissor and rounded clip carry over.
    pub(super) fn flush(&mut self, cause: GroupClose) {
        let order = &mut *self.order;
        order.occlusion.prune(self.out, order.cursors.quads);
        let mut group = self.out.quads.idx_mut(order.cursors.quads as usize..);
        let rows = group.slices_mut();
        order.runs.join(rows.key, rows.instance);
        let q_end = self.out.quads.len() as u32;
        let t_end = self.out.texts.len() as u32;
        let higher_end = PaintTier::ALL.map(|tier| self.out.draws_len(tier));
        if q_end > order.cursors.quads
            || t_end > order.cursors.texts
            || PaintTier::ALL
                .iter()
                .any(|&t| higher_end[t.idx()] > order.cursors.higher[t.idx()])
        {
            // Push higher-kind batches before the group so their `last_group` matches its eventual
            // index.
            let last_group = self.out.groups.len() as u32;
            for tier in PaintTier::ALL {
                let start = order.cursors.higher[tier.idx()];
                let end = higher_end[tier.idx()];
                if end > start {
                    self.out.batches_mut(tier).push(GroupBatch {
                        items: Span::try_from(start..end).unwrap(),
                        last_group,
                    });
                }
            }
            self.out.groups.push(DrawGroup {
                scissor: self.clip.scissor(),
                quads: Span::try_from(order.cursors.quads..q_end).unwrap(),
            });
            order.closes.edit(|tally| {
                tally.groups[cause as usize] += 1;
                tally.draws_before[cause as usize] += tally.draws;
            });
        }
        order.cursors = GroupCursors {
            quads: q_end,
            texts: t_end,
            higher: higher_end,
        };
        order.higher_kinds.clear();
        order.occlusion.clear();
        // Closed-batch text is group-scoped: past a group boundary those batches have rendered and
        // no longer gate quads. The open-batch grid spans groups.
        order.batch.closed_grid.clear();
        order.batch.pending_batch_cursor = self.out.text_batches.len();
    }

    /// Finalize the open text batch, if any. Called at batch-split events (quad or higher-kind
    /// overlap, strict-bounds mismatch, a backdrop's barrier). The grid fill is deferred to
    /// [`Self::closed_hit`].
    pub(super) fn close_batch(&mut self) {
        let Some(ClosedBatch { batch: b, scissor }) = self.order.batch.close() else {
            return;
        };
        let texts_end = self.out.texts.len() as u32;
        // Schedule-cursor invariants: `last_group` is non-decreasing in walk order, and `texts`
        // spans concatenate without gaps.
        debug_assert!(
            self.out
                .text_batches
                .last()
                .is_none_or(|prev| prev.last_group <= b.last_group),
        );
        debug_assert!(
            self.out
                .text_batches
                .last()
                .is_none_or(|prev| prev.texts.start + prev.texts.len == b.texts_start),
        );
        self.out.text_batches.push(TextBatch {
            texts: Span::try_from(b.texts_start..texts_end).unwrap(),
            last_group: b.last_group,
            // Already physical and clamped to every run's clip-narrowed bounds, so it is the
            // batch's GPU scissor, which clips a turned run; an unturned run clips its own glyphs.
            scissor,
        });
    }

    /// Return the open batch, opening one if none exists; refreshes `last_group` to the open
    /// group's eventual index.
    fn open_batch(&mut self) -> &mut OpenBatch {
        let last_group = self.out.groups.len() as u32;
        let texts_start = self.out.texts.len() as u32;
        let b = self.order.batch.open.get_or_insert(OpenBatch {
            texts_start,
            last_group,
            clip: TextClip::Whole,
        });
        b.last_group = last_group;
        b
    }

    /// Flush if a quad at `overlap` overlaps something in the group that would be reordered above
    /// it (quad is the lowest lane). Text is checked against the open batch's grid and batches
    /// closed in this group ([`Self::closed_hit`]); an open-batch hit also closes the batch so its
    /// text cannot coalesce forward and re-cover the quad. A batch renders at the end of its last
    /// group, so one whose last group is already closed paints below the quad and needs no flush of
    /// its own; a higher-kind draw in the group still does.
    #[inline]
    fn quad_forces_flush(&mut self, overlap: URect) {
        let mut renders_after = false;
        if self.order.batch.open_grid.any_overlap(overlap) {
            renders_after = self
                .order
                .batch
                .open
                .as_ref()
                .is_some_and(|b| self.renders_after_in_flight(b.last_group));
            self.close_batch();
        }
        if renders_after || self.closed_hit(overlap) || self.order.higher_kinds.any_overlap(overlap)
        {
            self.flush(GroupClose::PaintOrder);
        }
    }

    /// Whether a text batch whose last group is `last_group` renders at the end of the open group,
    /// after its draws, and not at the end of a group already closed.
    const fn renders_after_in_flight(&self, last_group: u32) -> bool {
        last_group as usize == self.out.groups.len()
    }

    /// `true` if `q` overlaps text of a batch closed in the open group that renders at its end.
    /// The first query hitting a pending batch scissor drains those batches into the closed grid;
    /// groups nothing probes never pay the fill.
    fn closed_hit(&mut self, q: URect) -> bool {
        let in_flight = self.out.groups.len();
        let batch = &mut self.order.batch;
        let pending = &self.out.text_batches[batch.pending_batch_cursor..];
        let renders_after = |b: &&TextBatch| b.last_group as usize == in_flight;
        if pending
            .iter()
            .filter(renders_after)
            .any(|b| b.scissor.intersects(q))
        {
            for b in pending.iter().filter(renders_after) {
                for ti in b.texts.range() {
                    batch.closed_grid.push(self.out.texts[ti].bounds);
                }
            }
            batch.pending_batch_cursor = self.out.text_batches.len();
        }
        batch.closed_grid.any_overlap(q)
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::renderer::frontend::composer::paint_order::{
        BatchState, CloseTally, GroupClose, PaintOrder,
    };

    /// The groups a frame closed for one cause, and the draws ordered after those closes, summed.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(crate) struct Closes {
        pub(crate) groups: u32,
        pub(crate) draws_after: u32,
    }

    /// A frame's closed groups, by cause.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(crate) struct GroupCloses {
        pub(crate) clip: Closes,
        pub(crate) paint_order: Closes,
        pub(crate) barrier: Closes,
        pub(crate) end: Closes,
    }

    impl CloseTally {
        fn of(&self, cause: GroupClose) -> Closes {
            let at = cause as usize;
            Closes {
                groups: self.groups[at],
                draws_after: self.groups[at] * self.draws - self.draws_before[at],
            }
        }
    }

    impl PaintOrder {
        pub(crate) fn scratch_bytes(&self) -> usize {
            let Self {
                batch:
                    BatchState {
                        open: _,
                        open_grid,
                        closed_grid,
                        pending_batch_cursor: _,
                    },
                higher_kinds,
                cursors: _,
                occlusion,
                runs,
                backdrop_break: _,
                closes: _,
            } = self;
            open_grid.scratch_bytes()
                + closed_grid.scratch_bytes()
                + higher_kinds.scratch_bytes()
                + occlusion.scratch_bytes()
                + runs.scratch_bytes()
        }

        /// The cover-containment tests the occlusion pruner ran.
        pub(crate) fn contains_tests(&self) -> u32 {
            self.occlusion.contains_tests()
        }

        /// Why the last frame's groups closed.
        pub(crate) fn group_closes(&self) -> GroupCloses {
            let tally = self.closes.get();
            GroupCloses {
                clip: tally.of(GroupClose::Clip),
                paint_order: tally.of(GroupClose::PaintOrder),
                barrier: tally.of(GroupClose::Barrier),
                end: tally.of(GroupClose::End),
            }
        }
    }
}
