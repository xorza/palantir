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
use crate::primitives::identity::pane_key::PaneKey;
use crate::scene::forest::Forest;
use std::mem;

/// Where raw rects stand in paint order: the rects from `start` up to the next mark came from
/// paint at `position` or later.
#[derive(Clone, Copy, Debug)]
struct RawMark {
    start: u32,
    position: u64,
}

/// The paint-order positions of a frame's raw rects, and the forward walk over its panes that
/// reads them. Marks are kept only on a frame with panes.
#[derive(Debug, Default)]
pub(super) struct PaneWalk {
    marks: Vec<RawMark>,
    on: bool,
    /// The last frame's pane keys, in paint order.
    held: Vec<PaneKey>,
    /// This frame's pane keys, in paint order.
    keys: Vec<PaneKey>,
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

    /// Start a frame: hold the last frame's pane keys and take this frame's, and mark from here
    /// on only if some plane of `cascade` has a pane. Every frame starts here, a whole one too,
    /// so the keys held are the last frame's whatever it painted.
    pub(super) fn start_frame(&mut self, cascade: &Cascade, forest: &Forest) {
        self.marks.clear();
        self.next_keys(
            forest
                .trees
                .iter_paint_order()
                .flat_map(|(plane, _)| cascade.planes[plane].panes.iter().map(|pane| pane.key)),
        );
        self.on = !self.keys.is_empty();
    }

    /// Hold the keys taken last and take `keys`, this frame's in paint order.
    fn next_keys(&mut self, keys: impl Iterator<Item = PaneKey>) {
        mem::swap(&mut self.held, &mut self.keys);
        self.keys.clear();
        self.keys.extend(keys);
    }

    /// The position of a row a node no longer paints: just before its first current row, as it
    /// stood somewhere before every row after it.
    pub(super) const fn gone_position(ord: usize, span: Span) -> u64 {
        Self::position(ord, span.start).saturating_sub(1)
    }

    /// Note that the rects pushed from `rects` on come from paint at `position` or later.
    pub(super) fn mark(&mut self, rects: usize, position: u64) {
        if !self.on {
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
    /// own position for the panes above it, and its key joins `dirty`. Returns `false` when more
    /// panes went stale than `dirty` holds.
    pub(super) fn walk(
        &mut self,
        cascade: &Cascade,
        forest: &Forest,
        rects: &mut Vec<Rect>,
        dirty: &mut DirtyPanes,
    ) -> bool {
        dirty.clear();
        if !self.on {
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
        self.walk_panes(panes, rects, dirty)
    }

    /// [`Self::walk`] over `panes`, each at its position, in paint order.
    fn walk_panes<'a>(
        &mut self,
        panes: impl Iterator<Item = (u64, &'a PaneRow)>,
        rects: &mut Vec<Rect>,
        dirty: &mut DirtyPanes,
    ) -> bool {
        for (i, (position, pane)) in panes.enumerate() {
            // A pane mostly keeps its place among the panes, so its own slot answers first.
            let held = self.held.get(i) == Some(&pane.key) || self.held.contains(&pane.key);
            if held && !self.touched_before(rects, position, pane.footprint) {
                continue;
            }
            self.mark(rects.len(), position);
            rects.push(pane.rect);
            if !dirty.push(pane.key) {
                return false;
            }
        }
        true
    }

    /// Whether a rect from paint before `position` touches `footprint`. Rects before the first
    /// mark have no position, so they count as before every pane.
    fn touched_before(&self, rects: &[Rect], position: u64, footprint: Rect) -> bool {
        let first = self.marks.first().map_or(rects.len(), |m| m.start as usize);
        if rects[..first].iter().any(|r| r.intersects(footprint)) {
            return true;
        }
        self.marks.iter().enumerate().any(|(i, mark)| {
            let end = self
                .marks
                .get(i + 1)
                .map_or(rects.len(), |next| next.start as usize);
            mark.position < position
                && rects[mark.start as usize..end]
                    .iter()
                    .any(|r| r.intersects(footprint))
        })
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::cascade::pane_row::PaneRow;
    use crate::damage::dirty_panes::DirtyPanes;
    use crate::damage::pane_walk::PaneWalk;
    #[cfg(test)]
    use crate::internals::scratch_report;
    use crate::primitives::geometry::rect::Rect;

    impl PaneWalk {
        #[cfg(test)]
        pub(crate) fn scratch_bytes(&self) -> usize {
            scratch_report::vec_bytes(&self.marks)
                + scratch_report::vec_bytes(&self.held)
                + scratch_report::vec_bytes(&self.keys)
        }

        /// Clear the marks, keeping the walk on.
        pub(crate) fn reset(&mut self) {
            self.marks.clear();
        }

        /// A walk that marks, as on a frame with panes.
        pub(crate) const fn marking() -> Self {
            Self {
                marks: Vec::new(),
                on: true,
                held: Vec::new(),
                keys: Vec::new(),
            }
        }

        /// [`Self::walk`] over `panes` at their positions, for a bench without a cascade.
        pub(crate) fn walk_over(
            &mut self,
            panes: &[(u64, PaneRow)],
            rects: &mut Vec<Rect>,
            dirty: &mut DirtyPanes,
        ) -> bool {
            dirty.clear();
            self.next_keys(panes.iter().map(|(_, pane)| pane.key));
            self.walk_panes(panes.iter().map(|(at, pane)| (*at, pane)), rects, dirty)
        }
    }
}
