//! Paint order: which draws may reorder within a group, and where a group or a text batch must
//! close so none that overlap does.

use crate::common::counters::TestOnly;
use crate::common::span::Span;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::urect::URect;
use crate::renderer::backdrop_captures::BackdropTicket;
use crate::renderer::frontend::composer::backdrop_break::BackdropBreak;
use crate::renderer::frontend::composer::clip_stack::ClipStack;
use crate::renderer::frontend::composer::higher_kind_rects::HigherKindRects;
use crate::renderer::frontend::composer::occlusion_pruner::OcclusionPruner;
use crate::renderer::frontend::composer::quad_runs::QuadRuns;
use crate::renderer::frontend::composer::rect_grid::RectGrid;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::backdrop_row::BackdropSource;
use crate::renderer::render_buffer::draw_run::{DrawRun, Lane};
use crate::renderer::render_buffer::paint_tier::PaintTier;
use crate::renderer::render_buffer::text_draw_row::TextDrawRow;
use glam::UVec2;

/// Why a group closed: a draw would have swapped paint order with one it overlaps, a backdrop's
/// barrier, or the session's end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GroupClose {
    PaintOrder,
    Barrier,
    End,
}

impl GroupClose {
    const COUNT: usize = 3;
}

/// A draw's extent and the part of it the clip in force leaves.
#[derive(Clone, Copy, Debug)]
pub(super) struct Seen {
    pub(super) bounds: URect,
    pub(super) visible: URect,
}

/// What a draw [`Ordering::admit`] let through takes: the pixels it paints, and its scissor
/// ([`Ordering::order`]), `None` for the whole viewport.
#[derive(Clone, Copy, Debug)]
pub(super) struct Admitted {
    pub(super) visible: URect,
    pub(super) scissor: Option<URect>,
}

/// Where a tier's run of the open group starts among its rows, the scissor it draws under, and
/// the pixels its draws paint.
#[derive(Clone, Copy, Debug)]
struct TierRun {
    start: u32,
    scissor: Option<URect>,
    bounds: URect,
}

/// A run to list: its lane's rows, the scissor it draws under and the pixels it paints, as a
/// [`DrawRun`] takes them.
#[derive(Clone, Copy, Debug)]
struct Listing {
    lane: Lane,
    rows: Span,
    scissor: Option<URect>,
    bounds: URect,
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
/// A group lists its runs as it closes, by [`Lane`], not in record order: its quads, the text
/// batches that end in it, then each [`PaintTier`] in `PaintTier::ALL` order; a text batch that
/// closes after its last group lists as it closes. That is safe iff no overlapping pair of draws
/// swaps record order, which [`Ordering::order`] enforces by closing the group, or the text batch,
/// where a draw follows an overlapping one of a lane that replays after it.
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
    /// The open group's index: the groups closed with a draw before it.
    group: u32,
    /// Each tier's open run in the open group, by [`PaintTier::idx`], `None` where it has drawn
    /// none: what the tier's next draw joins.
    open_runs: [Option<TierRun>; PaintTier::COUNT],
    /// Each tier's runs in the open group before its open one, by [`PaintTier::idx`], in record
    /// order.
    tier_runs: [Vec<TierRun>; PaintTier::COUNT],
    /// The runs listed of each lane, by [`Lane::idx`]: the next one's [`DrawRun::batch`].
    lane_runs: [u32; Lane::COUNT],
    /// Why this frame's groups closed, which the batching census reads.
    closes: TestOnly<CloseTally>,
}

/// Allocation-owning state for text batching. The open grid may span groups; the closed grid and
/// the batches waiting on the open group reset at each group boundary. `open` and `open_grid`
/// change together: the grid holds runs only while a batch is open.
#[derive(Debug, Default)]
struct BatchState {
    open: Option<OpenBatch>,
    open_grid: RectGrid,
    closed_grid: RectGrid,
    /// Batches closed in the open group that end in it, which list after its quads at its close.
    waiting: Vec<ClosedText>,
    /// The first of [`Self::waiting`] not yet indexed in `closed_grid`.
    waiting_indexed: usize,
    /// The union of the bounds of the waiting batches not yet indexed, which a probe tests before
    /// it indexes them; `None` where every one is.
    unindexed: Option<URect>,
    /// Where the next text run listed starts in `out.texts`: the runs list text in order.
    texts_listed: u32,
}

/// A text batch mid-accumulation, listed as a run by [`Ordering::close_batch`].
#[derive(Clone, Copy, Debug)]
struct OpenBatch {
    /// Cursor into `out.texts` where this batch's run span begins. Recorded, not derived from the
    /// previous span end (which it always equals), so the listing can assert they agree and catch a
    /// run pushed outside any batch.
    texts_start: u32,
    /// The last group whose text joined it, refreshed on every text push: a batch that ends in the
    /// open group lists after its quads, at its close.
    last_group: u32,
    /// Whether a run of it turns: its glyphs' quads fade half a pixel past each turned edge, so
    /// the batch draws under its runs' bounds, cut or not.
    turned: bool,
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

/// A text batch [`BatchState::close`] took, and the union of its runs' bounds.
#[derive(Clone, Copy, Debug)]
struct ClosedBatch {
    batch: OpenBatch,
    bounds: URect,
}

/// A closed text batch's runs, the union of their bounds, and the scissor they draw under: the
/// bounds where a run turns, as the backend clips no turned glyph on the CPU, and none otherwise.
#[derive(Clone, Copy, Debug)]
struct ClosedText {
    rows: Span,
    bounds: URect,
    scissor: Option<URect>,
}

/// Per-kind slice cursors for the open group: where its slice begins in each output buffer.
/// `texts` feeds only the did-anything-emit check, so a text-only group still counts and its
/// batch's `last_group` resolves.
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
        self.waiting.clear();
        self.waiting_indexed = 0;
        self.unindexed = None;
        self.texts_listed = 0;
    }

    /// Take the open batch and empty its grid, the two together.
    fn close(&mut self) -> Option<ClosedBatch> {
        let batch = self.open.take()?;
        let bounds = self.open_grid.union();
        self.open_grid.clear();
        Some(ClosedBatch { batch, bounds })
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
        self.group = 0;
        self.lane_runs = [0; Lane::COUNT];
        self.open_runs = [None; PaintTier::COUNT];
        for runs in &mut self.tier_runs {
            runs.clear();
        }
        self.closes.reset();
    }

    /// List `run` in `out`, after every run before it.
    fn list(&mut self, out: &mut RenderBuffer, run: Listing) {
        let Listing {
            lane,
            rows,
            scissor,
            bounds,
        } = run;
        debug_assert_ne!(rows.len, 0, "a run draws a row");
        debug_assert!(
            scissor.is_none_or(|scissor| bounds.clamp_to(scissor) == bounds),
            "a run paints inside its scissor"
        );
        let batch = &mut self.lane_runs[lane.idx()];
        out.runs.push(DrawRun {
            lane,
            rows,
            scissor,
            bounds,
            batch: *batch,
        });
        *batch += 1;
    }

    /// List the text batch `text` in `out`, whose runs follow the last batch listed.
    fn list_text(&mut self, out: &mut RenderBuffer, text: ClosedText) {
        debug_assert_eq!(
            text.rows.start, self.batch.texts_listed,
            "text batches list their runs in order, without a gap"
        );
        self.batch.texts_listed = text.rows.start + text.rows.len;
        self.list(
            out,
            Listing {
                lane: Lane::Text,
                rows: text.rows,
                scissor: text.scissor,
                bounds: text.bounds,
            },
        );
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
    /// [`Self::cull`] `bounds`, then [`Self::order`] a draw of `lane` there: the pixels it paints
    /// and its run's scissor, or `None` when it paints none, which then neither splits a batch nor
    /// registers a rect.
    #[inline(always)]
    pub(super) fn admit(&mut self, lane: Lane, bounds: URect) -> Option<Admitted> {
        let seen = self.cull(bounds)?;
        let scissor = self.order(lane, seen);
        Some(Admitted {
            visible: seen.visible,
            scissor,
        })
    }

    /// `bounds` and the part of them the clip in force leaves, `None` when that is empty.
    #[inline(always)]
    pub(super) fn cull(&self, bounds: URect) -> Option<Seen> {
        let visible = self.clip.clamped(bounds);
        (!visible.is_paint_empty()).then_some(Seen { bounds, visible })
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
    ///
    /// Returns the scissor the draw takes: a tier draw its run's ([`Self::run_scissor`]), a quad
    /// the clip in force, which the group's join trades for its run's; a text run's batch takes
    /// its own.
    // Always inlined: each caller's `lane` is a constant, so one arm remains and `visible` stays
    // in registers. As a call it took `visible` through memory, and `same_tier_image` ran 5 %
    // slower.
    #[inline(always)]
    pub(super) fn order(&mut self, lane: Lane, seen: Seen) -> Option<URect> {
        let visible = seen.visible;
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
        match lane {
            Lane::Text => None,
            // A quad takes the clip in force, which the group's join trades for its run's
            // scissor ([`QuadRuns::join`]).
            Lane::Quad => self.clip.scissor(),
            Lane::Tier(tier) => self.run_scissor(tier, seen),
        }
    }

    /// The scissor of the run of `tier` a draw `seen` joins: the open run's while that cuts the
    /// draw as its clip does, `bounds ∩ scissor = visible`, else a new run's, the clip in force
    /// where it cuts the draw and none where it does not. A scope whose clip cuts nothing thus
    /// costs no run, and one that cuts costs the runs of its cut draws. As Skia sets a scissor
    /// only for a draw its clip does not hold (`GrClip::IsInsideClip`).
    #[inline(always)]
    fn run_scissor(&mut self, tier: PaintTier, seen: Seen) -> Option<URect> {
        let Seen { bounds, visible } = seen;
        if let Some(run) = &mut self.order.open_runs[tier.idx()] {
            let cuts = match run.scissor {
                None => bounds,
                Some(scissor) => bounds.clamp_to(scissor),
            };
            if cuts == visible {
                run.bounds = run.bounds.union(visible);
                return run.scissor;
            }
        }
        self.open_tier_run(tier, seen)
    }

    /// Open a run of `tier` for a draw `seen` its open run cannot take, under the clip in force
    /// where that cuts the draw and none where it does not.
    fn open_tier_run(&mut self, tier: PaintTier, seen: Seen) -> Option<URect> {
        let cut = seen.bounds != seen.visible;
        debug_assert!(
            !cut || self.clip.scissor().is_some(),
            "only a clip cuts a draw"
        );
        let scissor = if cut { self.clip.scissor() } else { None };
        let run = TierRun {
            start: self.out.draws_len(tier),
            scissor,
            bounds: seen.visible,
        };
        if let Some(closed) = self.order.open_runs[tier.idx()].replace(run) {
            self.order.tier_runs[tier.idx()].push(closed);
        }
        scissor
    }

    /// Where a backdrop of `ticket` reads: its cached result where it holds one, else a
    /// capture at a break of the main pass. A capturing backdrop whose footprint misses everything
    /// painted since the last break shares it, and shows the clear when that is `0`, before any;
    /// else the text batch and the group close at a new barrier, so nothing before it coalesces,
    /// reorders or prunes against what follows: the main pass can break there and read the
    /// pixels the runs before it painted.
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
                let barrier = self.out.runs.len() as u32;
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
        b.turned |= row.spin.is_some();
        self.out.texts.push(row);
        self.order.batch.open_grid.push(bounds);
    }

    /// Note the quad just pushed as an occluder covering `cover`, for the group's prune.
    pub(super) fn record_opaque(&mut self, cover: Rect) {
        let idx = self.out.quads.len() as u32 - 1 - self.order.cursors.quads;
        self.order.occlusion.record_opaque(idx, cover);
    }

    /// Close the open group for `cause`: list its runs if it drew anything, advance the per-kind
    /// cursors and clear the overlap scratches. Scissor and rounded clip carry over.
    pub(super) fn flush(&mut self, cause: GroupClose) {
        let order = &mut *self.order;
        order.occlusion.prune(self.out, order.cursors.quads);
        let mut group = self.out.quads.idx_mut(order.cursors.quads as usize..);
        let rows = group.slices_mut();
        order.runs.join(rows.key, rows.scissor, rows.instance);
        let q_end = self.out.quads.len() as u32;
        let t_end = self.out.texts.len() as u32;
        let higher_end = PaintTier::ALL.map(|tier| self.out.draws_len(tier));
        let drew = q_end > order.cursors.quads
            || t_end > order.cursors.texts
            || PaintTier::ALL
                .iter()
                .any(|&t| higher_end[t.idx()] > order.cursors.higher[t.idx()]);
        debug_assert!(
            drew || order.batch.waiting.is_empty(),
            "a batch that ends in the group drew in it"
        );
        if drew {
            // The quads list a run a stretch of one scissor, as the join left them, each
            // painting what it shades inside the run's scissor.
            let physical = self.out.display.physical;
            let viewport = URect::new(0, 0, physical.x, physical.y);
            let mut run_start = order.cursors.quads;
            let mut bounds: Option<URect> = None;
            for at in order.cursors.quads..q_end {
                let at = at as usize;
                let scissor = self.out.quads.scissor()[at];
                let shaded =
                    URect::covering(order.runs.shaded()[at - order.cursors.quads as usize])
                        .clamp_to(scissor.unwrap_or(viewport));
                let run = bounds.map_or(shaded, |bounds| bounds.union(shaded));
                bounds = Some(run);
                if at + 1 == q_end as usize || self.out.quads.scissor()[at + 1] != scissor {
                    let rows = Span::try_from(run_start..at as u32 + 1).unwrap();
                    order.list(
                        self.out,
                        Listing {
                            lane: Lane::Quad,
                            rows,
                            scissor,
                            bounds: run,
                        },
                    );
                    run_start = at as u32 + 1;
                    bounds = None;
                }
            }
            for at in 0..order.batch.waiting.len() {
                let text = order.batch.waiting[at];
                order.list_text(self.out, text);
            }
            for tier in PaintTier::ALL {
                if let Some(open) = order.open_runs[tier.idx()].take() {
                    order.tier_runs[tier.idx()].push(open);
                }
                let rows_end = higher_end[tier.idx()];
                let runs = &order.tier_runs[tier.idx()];
                debug_assert!(
                    runs.first().map_or(rows_end, |run| run.start)
                        == order.cursors.higher[tier.idx()],
                    "a tier's first run of the group starts at its first row"
                );
                for at in 0..runs.len() {
                    let run = order.tier_runs[tier.idx()][at];
                    let end = order.tier_runs[tier.idx()]
                        .get(at + 1)
                        .map_or(rows_end, |next| next.start);
                    let rows = Span::try_from(run.start..end).unwrap();
                    order.list(
                        self.out,
                        Listing {
                            lane: Lane::Tier(tier),
                            rows,
                            scissor: run.scissor,
                            bounds: run.bounds,
                        },
                    );
                }
            }
            order.group += 1;
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
        order.open_runs = [None; PaintTier::COUNT];
        for runs in &mut order.tier_runs {
            runs.clear();
        }
        // The batches closed in the group have listed, so they no longer gate quads. The
        // open-batch grid spans groups.
        order.batch.closed_grid.clear();
        order.batch.waiting.clear();
        order.batch.waiting_indexed = 0;
        order.batch.unindexed = None;
    }

    /// Finalize the open text batch, if any. Called at batch-split events (quad or higher-kind
    /// overlap, strict-bounds mismatch, a backdrop's barrier). A batch that ends in the open group
    /// waits to list after its quads, at its close; one that ends in a group already closed lists
    /// now, as nothing since overlaps it ([`Self::order`] closed it at the first that did). The
    /// grid fill is deferred to [`Self::closed_hit`].
    pub(super) fn close_batch(&mut self) {
        let Some(ClosedBatch { batch: b, bounds }) = self.order.batch.close() else {
            return;
        };
        let text = ClosedText {
            rows: Span::try_from(b.texts_start..self.out.texts.len() as u32).unwrap(),
            // Already physical and clamped to every run's clip-narrowed bounds, so it is the
            // batch's GPU scissor where a run turns; an unturned run clips its own glyphs.
            bounds,
            scissor: b.turned.then_some(bounds),
        };
        if self.renders_after_in_flight(b.last_group) {
            let batch = &mut self.order.batch;
            batch.waiting.push(text);
            batch.unindexed = Some(
                batch
                    .unindexed
                    .map_or(text.bounds, |unindexed| unindexed.union(text.bounds)),
            );
        } else {
            self.order.list_text(self.out, text);
        }
    }

    /// Return the open batch, opening one if none exists; refreshes `last_group` to the open
    /// group's eventual index.
    fn open_batch(&mut self) -> &mut OpenBatch {
        let last_group = self.order.group;
        let texts_start = self.out.texts.len() as u32;
        let b = self.order.batch.open.get_or_insert(OpenBatch {
            texts_start,
            last_group,
            turned: false,
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
        last_group == self.order.group
    }

    /// `true` if `q` overlaps text of a batch closed in the open group that renders at its end.
    /// The first query hitting the union of the waiting batches not yet indexed drains them into
    /// the closed grid, so a group of many disjoint batches tests each query once against the union
    /// and once against the grid; groups nothing probes never pay the fill.
    fn closed_hit(&mut self, q: URect) -> bool {
        let BatchState {
            closed_grid,
            waiting,
            waiting_indexed,
            unindexed,
            ..
        } = &mut self.order.batch;
        if unindexed.is_some_and(|unindexed| unindexed.intersects(q)) {
            for text in &waiting[*waiting_indexed..] {
                for ti in text.rows.range() {
                    closed_grid.push(self.out.texts[ti].bounds);
                }
            }
            *waiting_indexed = waiting.len();
            *unindexed = None;
        }
        closed_grid.any_overlap(q)
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::renderer::frontend::composer::paint_order::{
        BatchState, CloseTally, ClosedText, GroupClose, PaintOrder, TierRun,
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
                        waiting,
                        waiting_indexed: _,
                        unindexed: _,
                        texts_listed: _,
                    },
                higher_kinds,
                cursors: _,
                occlusion,
                runs,
                backdrop_break: _,
                group: _,
                lane_runs: _,
                open_runs: _,
                tier_runs,
                closes: _,
            } = self;
            waiting.capacity() * size_of::<ClosedText>()
                + tier_runs
                    .iter()
                    .map(|runs| runs.capacity() * size_of::<TierRun>())
                    .sum::<usize>()
                + open_grid.scratch_bytes()
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
                paint_order: tally.of(GroupClose::PaintOrder),
                barrier: tally.of(GroupClose::Barrier),
                end: tally.of(GroupClose::End),
            }
        }
    }
}
