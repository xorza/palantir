//! Which backdrop panes the frame's damage reaches from below, so their captures go stale.
//!
//! # Decisions
//!
//! - **Damage takes a pane's rect, not its footprint.** Each raw damage rect has a place in paint
//!   order, laid as marks during the walk; a row that left takes the earliest place it could have
//!   had, a cost of one capture at worst. One forward walk over the panes in paint order is the
//!   fixed point: damage before a pane that touches its footprint adds the pane's rect, which
//!   counts as damage at the pane's place for the panes above it. The encode filter adds the
//!   footprint of each pane the damage reaches, so a capturing pane's footprint has its draws.
//!   Refused: Chromium's whole-surface rule, which repaints the surface for one pane.
//! - **A pane the last frame did not hold is stale.** Its key is its row among its owner's, so
//!   a row removed before it gives it a new key, and with it no held capture, while the row
//!   matcher sees the same row and damages nothing. Stale, its rect joins the damage and its
//!   footprint the encode filter, so the capture it takes reads every draw under it.

use crate::cascade::Cascade;
use crate::cascade::pane_row::PaneRow;
use crate::common::span::Span;
use crate::damage::dirty_panes::DirtyPanes;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::pane_keys::PaneKeys;
use crate::scene::forest::Forest;
use std::mem;
use std::ops::Range;
use tinyvec::ArrayVec;

/// Where raw rects stand in paint order: the rects from `start` up to the next mark came from
/// paint at `position` or later.
#[derive(Clone, Copy, Debug, Default)]
struct RawMark {
    start: u32,
    position: u64,
}

/// The raw rects of one mark, as the walk reads them: ordered by `key`, with their bounds, so a
/// footprint far from a run skips its rects.
#[derive(Clone, Debug)]
struct MarkRun {
    /// The mark's position plus one, or `0` for the rects before every mark, which stand before
    /// every pane: a run counts for a pane at `position` when `key <= position`.
    key: u64,
    rects: Range<u32>,
    bounds: Rect,
    /// The bounds of this run and every run before it in key order.
    prefix_bounds: Rect,
}

/// The paint-order positions of a frame's raw rects, and the forward walk over its panes that
/// reads them. Marks are kept only on a frame with panes.
#[derive(Debug, Default)]
pub(super) struct PaneWalk {
    marks: Vec<RawMark>,
    /// Walk scratch: the marks as runs, sorted by key.
    runs: Vec<MarkRun>,
    /// The last frame's pane keys.
    held: PaneKeys,
    /// This frame's pane keys.
    keys: PaneKeys,
    /// The panes this frame's damage left stale.
    dirty: DirtyPanes,
}

impl PaneWalk {
    /// The paint-order position of row `row` of the plane `ord`th in paint order: planes in order,
    /// rows in order within one, each row at an even position.
    pub(super) const fn position(ord: usize, row: u32) -> u64 {
        ((ord as u64) << 33) | (2 * row as u64)
    }

    /// The position of a node's rects: its first row's, or, for a node painting no row, just
    /// before the row its empty span starts at, which the next node's paint takes.
    pub(super) const fn node_position(ord: usize, span: Span) -> u64 {
        let position = Self::position(ord, span.start);
        if span.len == 0 {
            position.saturating_sub(1)
        } else {
            position
        }
    }

    /// Start a frame: hold the last frame's pane keys, take `cascade`'s, and drop the last
    /// frame's marks and stale panes. Every frame starts here, a whole one too, so the keys held
    /// are the last frame's whatever it painted, and a whole frame reports no stale pane.
    pub(super) fn start_frame(&mut self, cascade: &Cascade) {
        self.next_keys(&cascade.pane_keys);
    }

    fn next_keys(&mut self, keys: &PaneKeys) {
        self.marks.clear();
        self.dirty.clear();
        mem::swap(&mut self.held, &mut self.keys);
        self.keys.clone_from(keys);
    }

    /// The position of a row a node no longer paints: just before its first current row, as it
    /// stood somewhere before every row after it.
    pub(super) const fn gone_position(ord: usize, span: Span) -> u64 {
        Self::position(ord, span.start).saturating_sub(1)
    }

    /// Whether the frame has panes, so the rects pushed take marks.
    pub(super) const fn is_on(&self) -> bool {
        !self.keys.is_empty()
    }

    /// The panes this frame's damage left stale.
    pub(super) const fn dirty(&self) -> &DirtyPanes {
        &self.dirty
    }

    /// Note that the rects pushed from `rects` on come from paint at `position` or later.
    pub(super) fn mark(&mut self, rects: usize, position: u64) {
        if !self.is_on() {
            return;
        }
        let start = rects as u32;
        match self.marks.last_mut() {
            // The last mark covers no rect: this one replaces it.
            Some(last) if last.start == start => last.position = position,
            _ => self.marks.push(RawMark { start, position }),
        }
    }

    /// Walk the panes in paint order: a pane the last frame did not hold, or whose footprint a
    /// rect painted before it touches, has a stale capture, so its rect joins the damage, at its
    /// own position for the panes above it, and its key joins [`Self::dirty`]. Returns `false`
    /// when more panes went stale than it holds.
    pub(super) fn walk(
        &mut self,
        cascade: &Cascade,
        forest: &Forest,
        rects: &mut Vec<Rect>,
    ) -> bool {
        if !self.is_on() {
            return true;
        }
        let panes = forest
            .trees
            .iter_paint_order()
            .enumerate()
            .flat_map(|(ord, (plane, _))| {
                cascade.planes[plane]
                    .panes
                    .iter()
                    .map(move |pane| (Self::position(ord, pane.row), pane))
            });
        self.walk_panes(panes, rects)
    }

    /// [`Self::walk`] over `panes`, each at its position, in paint order.
    ///
    /// A position only approximates paint order: an ancestor's shape painted after its children
    /// has a lower row than theirs, so the walk meets positions in any order. The rects before
    /// the walk are sorted into runs by position once, with the bounds of each prefix, so the
    /// runs before a pane are the prefix a binary search finds, and that prefix's bounds reject
    /// a footprint far from all of it. The pane rects the walk pushes, at most
    /// [`DirtyPanes::CAP`], are read at their own positions. Cost: each pane reads the bounds of
    /// each run before it whose prefix its footprint touches, and the rects of a run whose own
    /// bounds it touches.
    fn walk_panes<'a>(
        &mut self,
        panes: impl Iterator<Item = (u64, &'a PaneRow)>,
        rects: &mut Vec<Rect>,
    ) -> bool {
        self.sort_runs(rects);
        let mut walked: ArrayVec<[RawMark; DirtyPanes::CAP]> = ArrayVec::new();
        for (position, pane) in panes {
            let fp = pane.footprint;
            let ready = self.runs.partition_point(|run| run.key <= position);
            let before = ready
                .checked_sub(1)
                .is_some_and(|last| self.runs[last].prefix_bounds.intersects(fp))
                && self.runs[..ready].iter().any(|run| {
                    run.bounds.intersects(fp)
                        && rects[run.rects.start as usize..run.rects.end as usize]
                            .iter()
                            .any(|r| r.intersects(fp))
                });
            let walked_before = walked
                .iter()
                .any(|mark| mark.position < position && rects[mark.start as usize].intersects(fp));
            if self.held.contains(pane.key) && !before && !walked_before {
                continue;
            }
            if !self.dirty.push(pane.key) {
                return false;
            }
            walked.push(RawMark {
                start: rects.len() as u32,
                position,
            });
            rects.push(pane.rect);
        }
        true
    }

    /// Lay the marks over `rects` as runs sorted by key, the rects before the first mark first,
    /// each with the bounds of the runs up to it.
    fn sort_runs(&mut self, rects: &[Rect]) {
        let bounds = |range: Range<usize>| {
            rects[range]
                .iter()
                .fold(Rect::ZERO, |bounds, r| bounds.union(*r))
        };
        self.runs.clear();
        let first = self.marks.first().map_or(rects.len(), |m| m.start as usize);
        self.runs.push(MarkRun {
            key: 0,
            rects: 0..first as u32,
            bounds: bounds(0..first),
            prefix_bounds: Rect::ZERO,
        });
        for (i, mark) in self.marks.iter().enumerate() {
            let end = self
                .marks
                .get(i + 1)
                .map_or(rects.len(), |next| next.start as usize);
            self.runs.push(MarkRun {
                key: mark.position + 1,
                rects: mark.start..end as u32,
                bounds: bounds(mark.start as usize..end),
                prefix_bounds: Rect::ZERO,
            });
        }
        self.runs.sort_unstable_by_key(|run| run.key);
        let mut prefix = Rect::ZERO;
        for run in &mut self.runs {
            prefix = prefix.union(run.bounds);
            run.prefix_bounds = prefix;
        }
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::cascade::pane_row::PaneRow;
    use crate::damage::pane_walk::PaneWalk;
    #[cfg(test)]
    use crate::internals::scratch_report;
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::identity::pane_keys::PaneKeys;

    impl PaneWalk {
        #[cfg(test)]
        pub(crate) fn scratch_bytes(&self) -> usize {
            scratch_report::vec_bytes(&self.marks)
                + scratch_report::vec_bytes(&self.runs)
                + self.held.scratch_bytes()
                + self.keys.scratch_bytes()
        }

        /// [`Self::start_frame`] over `keys`, for a bench without a cascade.
        pub(crate) fn start_over(&mut self, keys: &PaneKeys) {
            self.next_keys(keys);
        }

        /// [`Self::walk`] over `panes` at their positions, for a bench without a cascade.
        pub(crate) fn walk_over(
            &mut self,
            panes: &[(u64, PaneRow)],
            rects: &mut Vec<Rect>,
        ) -> bool {
            self.walk_panes(panes.iter().map(|(at, pane)| (*at, pane)), rects)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::cascade::pane_row::PaneRow;
    use crate::damage::dirty_panes::DirtyPanes;
    use crate::damage::pane_walk::PaneWalk;
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::identity::pane_key::PaneKey;
    use crate::primitives::identity::pane_keys::PaneKeys;
    use crate::primitives::identity::widget_id::WidgetId;
    use crate::shape::paint::blur_plan::BlurPlan;

    /// Panes `a`, `b` and `c` at positions 10, 20 and 30, each a 10 px square 100 px from the
    /// next, its footprint the square itself.
    fn panes() -> Vec<(u64, PaneRow)> {
        (0..3u32)
            .map(|i| {
                let rect = Rect::new(i as f32 * 100.0, 0.0, 10.0, 10.0);
                let pane = PaneRow {
                    key: PaneKey {
                        owner: WidgetId::from_hash(u64::from(i)),
                        shape: 0,
                    },
                    row: 5 * (i + 1),
                    rect,
                    plan: BlurPlan::new(1.0),
                    footprint: rect,
                };
                (10 * u64::from(i + 1), pane)
            })
            .collect()
    }

    /// The panes `marked` leaves stale on a frame that holds all three, where each `(position,
    /// x)` lays a 4 px rect at `x` painted at `position`, `None` before every mark.
    fn stale(marked: &[(Option<u64>, f32)]) -> DirtyPanes {
        let panes = panes();
        let mut keys = PaneKeys::default();
        keys.refill(panes.iter().map(|(_, pane)| pane.key));
        let mut walk = PaneWalk::default();
        walk.start_over(&keys);
        walk.start_over(&keys);
        let mut rects = Vec::new();
        for &(position, x) in marked {
            if let Some(position) = position {
                walk.mark(rects.len(), position);
            }
            rects.push(Rect::new(x, 2.0, 4.0, 4.0));
        }
        assert!(walk.walk_over(&panes, &mut rects));
        *walk.dirty()
    }

    /// A rect stales the pane under it only when painted before it, whatever order the marks
    /// came in: at 25 over `a` (at 10) it is after `a`, at 5 over `b` (at 20) before `b`, at 15
    /// over `c` (at 30) before `c`. A rect before every mark is before every pane. A held pane
    /// nothing touches stays clean.
    #[test]
    fn a_rect_stales_only_the_panes_painted_after_it() {
        let key = |i: usize| panes()[i].1.key;
        assert_eq!(stale(&[]), DirtyPanes::default());
        assert_eq!(stale(&[(Some(25), 2.0)]), DirtyPanes::default(), "after a");
        assert_eq!(
            stale(&[(Some(25), 2.0), (Some(5), 102.0), (Some(15), 202.0)]),
            DirtyPanes::from_iter([key(1), key(2)]),
        );
        assert_eq!(
            stale(&[(None, 2.0), (Some(25), 102.0)]),
            DirtyPanes::from_iter([key(0)]),
            "before every mark",
        );
    }

    /// A pane the last frame did not hold is stale, and its rect stands before every later pane:
    /// `a`, new, grown to reach `b`'s footprint, stales `b` too.
    #[test]
    fn a_new_panes_rect_stales_the_panes_above_it() {
        let mut panes = panes();
        panes[0].1.rect = Rect::new(0.0, 0.0, 105.0, 10.0);
        let mut held = PaneKeys::default();
        held.refill(panes[1..].iter().map(|(_, pane)| pane.key));
        let mut keys = PaneKeys::default();
        keys.refill(panes.iter().map(|(_, pane)| pane.key));
        let mut walk = PaneWalk::default();
        walk.start_over(&held);
        walk.start_over(&keys);
        let mut rects = Vec::new();
        assert!(walk.walk_over(&panes, &mut rects));
        assert_eq!(
            *walk.dirty(),
            DirtyPanes::from_iter([panes[0].1.key, panes[1].1.key])
        );
        assert_eq!(rects, [panes[0].1.rect, panes[1].1.rect]);
    }
}
