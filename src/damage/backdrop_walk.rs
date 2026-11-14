//! Which backdrops the frame's damage reaches from below, so their captures go stale.
//!
//! # Decisions
//!
//! - **Damage takes a backdrop's rect, not its footprint.** Each raw damage rect has a place in
//!   paint order, laid as marks during the walk; a row that left takes the earliest place it could
//!   have had, a cost of one capture at worst. One forward walk over the backdrops in paint order
//!   is the fixed point: damage before a backdrop that touches its footprint adds the backdrop's
//!   rect, which counts as damage at the backdrop's place for the backdrops above it. The encode
//!   filter adds the footprint of each backdrop the damage reaches, so a capturing backdrop's
//!   footprint has its draws. Refused: Chromium's whole-surface rule, which repaints the surface
//!   for one backdrop.
//! - **A backdrop the last frame did not hold is stale.** Its key is its row among its owner's, so
//!   a row removed before it gives it a new key, and with it no held capture, while the row
//!   matcher sees the same row and damages nothing. Stale, its rect joins the damage and its
//!   footprint the encode filter, so the capture it takes reads every draw under it.

use crate::cascade::Cascade;
use crate::cascade::plane_backdrop::PlaneBackdrop;
use crate::common::span::Span;
use crate::damage::dirty_backdrops::DirtyBackdrops;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::backdrop_keys::BackdropKeys;
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
    /// every backdrop: a run counts for a backdrop at `position` when `key <= position`. A mark at
    /// [`BackdropWalk::BEFORE_EVERY`] keys 1, at or below every row's position.
    key: u64,
    rects: Range<u32>,
    bounds: Rect,
    /// The bounds of this run and every run before it in key order.
    prefix_bounds: Rect,
}

/// The paint-order positions of a frame's raw rects, and the forward walk over its backdrops that
/// reads them. Marks are kept only on a frame with backdrops.
#[derive(Debug, Default)]
pub(super) struct BackdropWalk {
    marks: Vec<RawMark>,
    /// Walk scratch: the marks as runs, sorted by key.
    runs: Vec<MarkRun>,
    /// The last frame's backdrop keys.
    held: BackdropKeys,
    /// This frame's backdrop keys.
    keys: BackdropKeys,
    /// The backdrops this frame's damage left stale.
    dirty: DirtyBackdrops,
}

impl BackdropWalk {
    /// The position of rects that stand before every backdrop, as the removed-widget tail's, whose
    /// rows have no place this frame. Below every row's position and every one just before a row.
    pub(super) const BEFORE_EVERY: u64 = 0;

    /// The paint-order position of row `row` of the plane `ord`th in paint order: planes in order,
    /// rows in order within one, each row at an even position from 2, so the odd one below each is
    /// free for the place just before it, the first row's included, and 0 for
    /// [`Self::BEFORE_EVERY`]. `2 * row + 2` for any `u32` row stays below `1 << 34`.
    pub(super) const fn position(ord: usize, row: u32) -> u64 {
        ((ord as u64) << 34) | (2 * row as u64 + 2)
    }

    /// The position of a node's rects: its first row's, or, for a node painting no row, just
    /// before the row its empty span starts at, which the next node's paint takes.
    pub(super) const fn node_position(ord: usize, span: Span) -> u64 {
        let position = Self::position(ord, span.start);
        if span.len == 0 {
            position - 1
        } else {
            position
        }
    }

    /// Start a frame: hold the last frame's backdrop keys, take `cascade`'s, and drop the last
    /// frame's marks and stale backdrops. Every frame starts here, a whole one too, so the keys
    /// held are the last frame's whatever it painted, and a whole frame reports no stale backdrop.
    pub(super) fn start_frame(&mut self, cascade: &Cascade) {
        self.next_keys(&cascade.backdrop_keys);
    }

    fn next_keys(&mut self, keys: &BackdropKeys) {
        self.marks.clear();
        self.dirty.clear();
        mem::swap(&mut self.held, &mut self.keys);
        self.keys.clone_from(keys);
    }

    /// The position of a row a node no longer paints: just before its first current row, as it
    /// stood somewhere before every row after it.
    pub(super) const fn gone_position(ord: usize, span: Span) -> u64 {
        Self::position(ord, span.start) - 1
    }

    /// Whether the frame has backdrops, so the rects pushed take marks.
    pub(super) const fn is_on(&self) -> bool {
        !self.keys.is_empty()
    }

    /// The backdrops this frame's damage left stale.
    pub(super) const fn dirty(&self) -> &DirtyBackdrops {
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

    /// Walk the backdrops in paint order: a backdrop the last frame did not hold, or whose
    /// footprint a rect painted before it touches, has a stale capture, so its rect joins the
    /// damage, at its own position for the backdrops above it, and its key joins [`Self::dirty`].
    /// Returns `false` when more backdrops went stale than it holds.
    pub(super) fn walk(
        &mut self,
        cascade: &Cascade,
        forest: &Forest,
        rects: &mut Vec<Rect>,
    ) -> bool {
        if !self.is_on() {
            return true;
        }
        let backdrops =
            forest
                .trees
                .iter_paint_order()
                .enumerate()
                .flat_map(|(ord, (plane, _))| {
                    cascade.planes[plane]
                        .backdrops
                        .iter()
                        .map(move |backdrop| (Self::position(ord, backdrop.paint_row), backdrop))
                });
        self.walk_backdrops(backdrops, rects)
    }

    /// [`Self::walk`] over `backdrops`, each at its position, in paint order.
    ///
    /// A position only approximates paint order: a backdrop stands past every row painted before it
    /// ([`PlaneBackdrop::paint_row`]), but a row its node paints after it can stand below it, and
    /// the marks come in any order. The rects before the walk are sorted into runs by position
    /// once, with the bounds of each prefix, so the runs before a backdrop are the prefix a binary
    /// search finds, and that prefix's bounds reject a footprint far from all of it. The backdrop
    /// rects the walk pushes, at most [`DirtyBackdrops::CAP`], are read at their own positions.
    /// Cost: each backdrop reads the bounds of each run before it whose prefix its footprint
    /// touches, and the rects of a run whose own bounds it touches.
    fn walk_backdrops<'a>(
        &mut self,
        backdrops: impl Iterator<Item = (u64, &'a PlaneBackdrop)>,
        rects: &mut Vec<Rect>,
    ) -> bool {
        self.sort_runs(rects);
        let mut walked: ArrayVec<[RawMark; DirtyBackdrops::CAP]> = ArrayVec::new();
        for (position, backdrop) in backdrops {
            let fp = backdrop.footprint;
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
            if self.held.contains(backdrop.key) && !before && !walked_before {
                continue;
            }
            if !self.dirty.push(backdrop.key) {
                return false;
            }
            walked.push(RawMark {
                start: rects.len() as u32,
                position,
            });
            rects.push(backdrop.rect);
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
    use crate::cascade::plane_backdrop::PlaneBackdrop;
    use crate::damage::backdrop_walk::BackdropWalk;
    #[cfg(test)]
    use crate::internals::scratch_report;
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::identity::backdrop_key::BackdropKey;
    use crate::primitives::identity::backdrop_keys::BackdropKeys;

    impl BackdropWalk {
        /// Mark backdrop `key` stale in this frame, as a walk that found damage under it does.
        ///
        /// # Panics
        ///
        /// Panics past
        /// [`DirtyBackdrops::CAP`](crate::damage::dirty_backdrops::DirtyBackdrops::CAP), which a
        /// real walk answers by painting whole.
        pub(crate) fn force_stale(&mut self, key: BackdropKey) {
            assert!(
                self.dirty.push(key),
                "more stale backdrops than a frame holds"
            );
        }

        #[cfg(test)]
        pub(crate) fn scratch_bytes(&self) -> usize {
            scratch_report::vec_bytes(&self.marks)
                + scratch_report::vec_bytes(&self.runs)
                + self.held.scratch_bytes()
                + self.keys.scratch_bytes()
        }

        /// [`Self::start_frame`] over `keys`, for a bench without a cascade.
        pub(crate) fn start_over(&mut self, keys: &BackdropKeys) {
            self.next_keys(keys);
        }

        /// [`Self::walk`] over `backdrops` at their positions, for a bench without a cascade.
        pub(crate) fn walk_over(
            &mut self,
            backdrops: &[(u64, PlaneBackdrop)],
            rects: &mut Vec<Rect>,
        ) -> bool {
            self.walk_backdrops(
                backdrops.iter().map(|(at, backdrop)| (*at, backdrop)),
                rects,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::cascade::plane_backdrop::PlaneBackdrop;
    use crate::common::span::Span;
    use crate::damage::backdrop_walk::BackdropWalk;
    use crate::damage::dirty_backdrops::DirtyBackdrops;
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::identity::backdrop_key::BackdropKey;
    use crate::primitives::identity::backdrop_keys::BackdropKeys;
    use crate::primitives::identity::widget_id::WidgetId;
    use crate::shape::paint::blur_plan::BlurPlan;

    /// Backdrops `a`, `b` and `c` at positions 10, 20 and 30, each a 10 px square 100 px from the
    /// next, its footprint the square itself.
    fn backdrops() -> Vec<(u64, PlaneBackdrop)> {
        (0..3u32)
            .map(|i| {
                let rect = Rect::new(i as f32 * 100.0, 0.0, 10.0, 10.0);
                let backdrop = PlaneBackdrop {
                    key: BackdropKey {
                        owner: WidgetId::from_hash(u64::from(i)),
                        shape: 0,
                    },
                    row: 5 * (i + 1),
                    paint_row: 5 * (i + 1),
                    rect,
                    plan: BlurPlan::new(1.0),
                    footprint: rect,
                };
                (10 * u64::from(i + 1), backdrop)
            })
            .collect()
    }

    /// The backdrops `marked` leaves stale on a frame that holds all three, where each `(position,
    /// x)` lays a 4 px rect at `x` painted at `position`, `None` before every mark.
    fn stale(marked: &[(Option<u64>, f32)]) -> DirtyBackdrops {
        let backdrops = backdrops();
        let mut keys = BackdropKeys::default();
        keys.refill(backdrops.iter().map(|(_, backdrop)| backdrop.key));
        let mut walk = BackdropWalk::default();
        walk.start_over(&keys);
        walk.start_over(&keys);
        let mut rects = Vec::new();
        for &(position, x) in marked {
            if let Some(position) = position {
                walk.mark(rects.len(), position);
            }
            rects.push(Rect::new(x, 2.0, 4.0, 4.0));
        }
        assert!(walk.walk_over(&backdrops, &mut rects));
        *walk.dirty()
    }

    /// A rect stales the backdrop under it only when painted before it, whatever order the marks
    /// came in: at 25 over `a` (at 10) it is after `a`, at 5 over `b` (at 20) before `b`, at 15
    /// over `c` (at 30) before `c`. A rect before every mark is before every backdrop. A held
    /// backdrop nothing touches stays clean.
    #[test]
    fn a_rect_stales_only_the_backdrops_painted_after_it() {
        let key = |i: usize| backdrops()[i].1.key;
        assert_eq!(stale(&[]), DirtyBackdrops::default());
        assert_eq!(
            stale(&[(Some(25), 2.0)]),
            DirtyBackdrops::default(),
            "after a"
        );
        assert_eq!(
            stale(&[(Some(25), 2.0), (Some(5), 102.0), (Some(15), 202.0)]),
            DirtyBackdrops::from_iter([key(1), key(2)]),
        );
        assert_eq!(
            stale(&[(None, 2.0), (Some(25), 102.0)]),
            DirtyBackdrops::from_iter([key(0)]),
            "before every mark",
        );
    }

    /// Every place a mark takes at the first row of the lowest plane is ordered: before every
    /// backdrop (0), a removed node's and an empty one's just before the row (1), the row (2). So
    /// a rect marked before every backdrop stales one at that first row, as the removed-widget
    /// tail must, and one marked at the row itself does not.
    #[test]
    fn the_first_row_stands_after_every_place_before_it() {
        let empty = Span::new(0, 0);
        let first = BackdropWalk::position(0, 0);
        assert_eq!(
            [
                BackdropWalk::BEFORE_EVERY,
                BackdropWalk::gone_position(0, empty),
                BackdropWalk::node_position(0, empty),
                BackdropWalk::node_position(0, Span::new(0, 1)),
                first,
            ],
            [0, 1, 1, 2, 2]
        );
        assert_eq!(
            BackdropWalk::position(1, u32::MAX),
            (1 << 34) | (1 << 33),
            "the largest row stays inside its plane"
        );

        let staled = |mark: u64| {
            let rect = Rect::new(0.0, 0.0, 10.0, 10.0);
            let backdrop = PlaneBackdrop {
                key: BackdropKey {
                    owner: WidgetId::from_hash("first"),
                    shape: 0,
                },
                row: 0,
                paint_row: 0,
                rect,
                plan: BlurPlan::new(1.0),
                footprint: rect,
            };
            let mut keys = BackdropKeys::default();
            keys.refill([backdrop.key]);
            let mut walk = BackdropWalk::default();
            walk.start_over(&keys);
            walk.start_over(&keys);
            let mut rects = Vec::new();
            walk.mark(rects.len(), mark);
            rects.push(Rect::new(2.0, 2.0, 4.0, 4.0));
            assert!(walk.walk_over(&[(first, backdrop)], &mut rects));
            walk.dirty().contains(backdrop.key)
        };
        assert!(
            staled(BackdropWalk::BEFORE_EVERY),
            "the tail stands before it"
        );
        assert!(staled(BackdropWalk::gone_position(0, empty)));
        assert!(
            !staled(first),
            "a rect at the row paints after the backdrop's capture"
        );
    }

    /// A backdrop the last frame did not hold is stale, and its rect stands before every later
    /// backdrop: `a`, new, grown to reach `b`'s footprint, stales `b` too.
    #[test]
    fn a_new_backdrops_rect_stales_the_backdrops_above_it() {
        let mut backdrops = backdrops();
        backdrops[0].1.rect = Rect::new(0.0, 0.0, 105.0, 10.0);
        let mut held = BackdropKeys::default();
        held.refill(backdrops[1..].iter().map(|(_, backdrop)| backdrop.key));
        let mut keys = BackdropKeys::default();
        keys.refill(backdrops.iter().map(|(_, backdrop)| backdrop.key));
        let mut walk = BackdropWalk::default();
        walk.start_over(&held);
        walk.start_over(&keys);
        let mut rects = Vec::new();
        assert!(walk.walk_over(&backdrops, &mut rects));
        assert_eq!(
            *walk.dirty(),
            DirtyBackdrops::from_iter([backdrops[0].1.key, backdrops[1].1.key])
        );
        assert_eq!(rects, [backdrops[0].1.rect, backdrops[1].1.rect]);
    }
}
