//! Paint order: which draws may reorder within a group, and where a group or a text batch must
//! close so none that overlap does.

use crate::common::span::Span;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::urect::URect;
use crate::renderer::frontend::composer::clip_stack::{ClipFrame, ClipStack};
use crate::renderer::frontend::composer::higher_kind_rects::HigherKindRects;
use crate::renderer::frontend::composer::occlusion_pruner::OcclusionPruner;
use crate::renderer::frontend::composer::pane_break::PaneBreak;
use crate::renderer::frontend::composer::rect_grid::RectGrid;
use crate::renderer::pane_captures::PaneTicket;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::backdrop_row::PaneSource;
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

/// The retained state of paint order: the open text batch, the higher-kind rects, where the open
/// group's slices begin, the occluders the group prunes with, and the pane break.
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
    /// The last break the main pass takes for a pane, and what is painted after it.
    pane_break: PaneBreak,
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
    /// `true` once a strict run has joined: one whose ancestor clip cuts its full unclipped extent.
    /// The batch scissor (`open_grid.union`) must then equal that bound, so later runs join only
    /// with matching `bounds`; the text shader has no per-instance clip.
    strict: bool,
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
        let scissor = self.open_grid.union;
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
        self.pane_break.open(0);
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
    /// as painted since the pane break.
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
                    self.flush();
                }
            }
            Lane::Tier(tier) => {
                if self.order.batch.open_grid.any_overlap(visible) {
                    self.close_batch();
                }
                if self.order.higher_kinds.conflicts(tier, visible) {
                    self.flush();
                }
                self.order.higher_kinds.push(tier, visible);
            }
        }
        self.order.pane_break.paint(visible);
    }

    /// Where a pane of `ticket` reads its backdrop: its cached result where it holds one, else a
    /// capture at a break of the main pass. A capturing pane whose footprint misses everything
    /// painted since the last break shares it, and shows the clear when that is `0`, before any;
    /// else the text batch and the group close at a new barrier, so nothing before it coalesces,
    /// reorders or prunes against what follows: the main pass can break there and read the
    /// pixels the groups before it painted.
    pub(super) fn pane_source(&mut self, ticket: PaneTicket) -> PaneSource {
        if !ticket.captures {
            return PaneSource::Held;
        }
        let barrier = self
            .order
            .pane_break
            .shared(ticket.footprint)
            .unwrap_or_else(|| {
                self.close_batch();
                self.flush();
                let barrier = self.out.groups.len() as u32;
                self.order.pane_break.open(barrier);
                barrier
            });
        if barrier == 0 {
            PaneSource::Clear
        } else {
            PaneSource::Capture { barrier }
        }
    }

    /// Push the text run `row` into the open batch, opening one where none is or where its scissor
    /// cannot hold the run: a `strict` run, cut by an ancestor clip, batches only with peers of
    /// identical bounds, as the text shader has no per-instance clip. The batch spans groups,
    /// the one draw whose order is not group-scoped.
    pub(super) fn push_text(&mut self, row: TextDrawRow, strict: bool) {
        let bounds = row.bounds;
        if let Some(b) = self.order.batch.open.as_ref()
            && (b.strict || strict)
            && self.order.batch.open_grid.union != bounds
        {
            self.close_batch();
        }
        // Opened before the push, so `texts_start` takes this run's index.
        let b = self.open_batch();
        b.strict |= strict;
        self.out.texts.push(row);
        self.order.batch.open_grid.push(bounds);
    }

    /// Note the quad just pushed as an occluder covering `cover`, for the group's prune.
    pub(super) fn record_opaque(&mut self, cover: Rect) {
        let idx = self.out.quads.len() as u32 - 1 - self.order.cursors.quads;
        self.order.occlusion.record_opaque(idx, cover);
    }

    /// Close what the clip in force owns, if `next` differs. Chains compare by value, so a
    /// redundant push/pop is a no-op. Runs before the stack moves, since [`Self::flush`] stamps the
    /// closing group with the stack top.
    pub(super) fn break_for_clip(&mut self, next: Option<ClipFrame>) {
        let next_chain = next.map_or_else(Span::default, |frame| frame.chain);
        let chain_changed = !self.out.chains_equal(next_chain, self.clip.chain());
        if chain_changed {
            // The stencil mask stack follows the active chain; close before the group transition,
            // while the stack top still names the batch's chain.
            self.close_batch();
        }
        if next.map(|frame| frame.scissor) != self.clip.scissor() || chain_changed {
            self.flush();
        }
    }

    /// Close the open group: push a `DrawGroup` if anything was emitted, advance the per-kind
    /// cursors and clear the overlap scratches. Scissor and rounded clip carry over.
    pub(super) fn flush(&mut self) {
        let order = &mut *self.order;
        order.occlusion.prune(self.out, order.cursors.quads);
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
                        items: (start..end).into(),
                        last_group,
                    });
                }
            }
            self.out.groups.push(DrawGroup {
                scissor: self.clip.scissor(),
                rounded_clips: self.clip.chain(),
                quads: (order.cursors.quads..q_end).into(),
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

    /// Finalize the open text batch, if any. Called at batch-split events (rounded-clip change,
    /// higher-kind append, strict-bounds mismatch). The grid fill is deferred to
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
            texts: (b.texts_start..texts_end).into(),
            last_group: b.last_group,
            // Already physical and clamped to every run's clip-narrowed bounds, so it is the
            // batch's GPU scissor (the text backend has no per-run clipping).
            scissor,
            // Every close site runs while the outgoing clip is still the stack top.
            rounded_clips: self.clip.chain(),
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
            strict: false,
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
            self.flush();
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
    use crate::renderer::frontend::composer::paint_order::{BatchState, PaintOrder};

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
                pane_break: _,
            } = self;
            open_grid.scratch_bytes()
                + closed_grid.scratch_bytes()
                + higher_kinds.scratch_bytes()
                + occlusion.scratch_bytes()
        }

        /// The cover-containment tests the occlusion pruner ran.
        pub(crate) fn contains_tests(&self) -> u32 {
            self.occlusion.contains_tests()
        }
    }
}
