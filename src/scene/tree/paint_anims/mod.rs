//! Paint-only animations: the per-shape contract (`PaintAnimation` /
//! `PaintMod`) and the per-tree registry that stores it.
//!
//! They don't affect layout, hit-test or structure. Widgets register one via
//! `Ui::add_shape_animated`; the encoder samples it at paint time and folds
//! the [`PaintMod`] into the shape's brush. `Forest::min_paint_anim_wake`
//! (called by `FrameCycle::run`) feeds each `next_wake` to the wake queue, so
//! widgets never call `request_repaint_after` for them.
//!
//! Unlike `crate::animation`, sampling is a pure function of `now` at encode
//! time, so irregular `dt` doesn't drift.
//!
//! The registry stores only live entries, sorted by shape index; the
//! encoder's monotonic cursor skips culled ranges.
//!
//! An animation drives alpha, rotation, or both. Translation and scale would
//! need a swept union the cascade can't bound without sampling; a rotation's
//! cover is the same square at every angle.

#[cfg(feature = "bench")]
pub(crate) mod bench;
pub mod curves;
pub(crate) mod paint_animation;
pub(crate) mod paint_mod;

use crate::scene::tree::node_id::NodeId;
use crate::scene::tree::paint_anims::paint_animation::PaintAnimation;
use crate::scene::tree::paint_anims::paint_mod::PaintMod;
use std::time::Duration;

const CURSOR_END: u64 = u64::MAX;

/// One row per registered paint animation.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PaintAnimEntry {
    pub(crate) anim: PaintAnimation,
    /// Index into `Tree::shapes.records`. Strictly increasing across
    /// [`PaintAnims::entries`] (append-only recording), which enables
    /// [`PaintAnims::rotates`]'s binary search and [`PaintAnimCursor`].
    pub(crate) shape_idx: u32,
    /// Paint-arena row of the shape inside its owner's `node_spans` span, from
    /// `OpenFrame::paint_rows` at registration, so damage can index
    /// `paint_arena.rows[node_span.start + row]` directly.
    pub(crate) row: u32,
    /// The node owning the shape, for direct `node_spans[node]` lookup.
    pub(crate) node: NodeId,
}

/// Per-tree sparse paint-animation registry, cleared per frame.
#[derive(Debug, Default)]
pub(crate) struct PaintAnims {
    /// Live entries in registration (shape) order, so `shape_idx` increases.
    pub(crate) entries: Vec<PaintAnimEntry>,
}

impl PaintAnims {
    /// Reset for a fresh recording frame; capacity retained.
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }

    /// Register one animation against the shape it was recorded on.
    pub(crate) fn push_entry(&mut self, entry: PaintAnimEntry) {
        debug_assert!(
            self.entries
                .last()
                .is_none_or(|last| last.shape_idx < entry.shape_idx),
            "paint animation shape indices must be strictly increasing",
        );
        self.entries.push(entry);
    }

    /// Whether the shape at `shape_idx` paints under a rotation. Needs no `now`:
    /// culling and damage use the swept square, identical at every angle.
    pub(crate) fn rotates(&self, shape_idx: u32) -> bool {
        self.entries
            .binary_search_by_key(&shape_idx, |entry| entry.shape_idx)
            .is_ok_and(|i| self.entries[i].anim.rotates())
    }

    pub(crate) fn cursor(&self) -> PaintAnimCursor<'_> {
        PaintAnimCursor {
            entries: &self.entries,
            next: 0,
            next_shape: next_shape(&self.entries, 0),
            #[cfg(debug_assertions)]
            last_sampled: None,
        }
    }
}

/// `entries[next]`'s shape index, or [`CURSOR_END`] past the last row.
#[inline]
fn next_shape(entries: &[PaintAnimEntry], next: usize) -> u64 {
    entries
        .get(next)
        .map_or(CURSOR_END, |entry| u64::from(entry.shape_idx))
}

/// Monotonic encoder lookup over the sparse animation rows.
#[derive(Debug)]
pub(crate) struct PaintAnimCursor<'a> {
    entries: &'a [PaintAnimEntry],
    next: usize,
    next_shape: u64,
    /// Previous [`Self::sample`] argument, checking monotonicity in debug. A
    /// backwards walk gives no failure signal: it answers `IDENTITY` and leaves
    /// the skipped registration consumed.
    #[cfg(debug_assertions)]
    last_sampled: Option<u32>,
}

impl PaintAnimCursor<'_> {
    /// `shape_idx` must increase between calls. Jumps are allowed because
    /// viewport and damage culling can skip whole shape ranges.
    #[inline]
    pub(crate) fn sample(&mut self, shape_idx: u32, now: Duration) -> PaintMod {
        #[cfg(debug_assertions)]
        {
            debug_assert!(
                self.last_sampled.is_none_or(|last| shape_idx > last),
                "paint-anim sampling must be monotonic — sampled {shape_idx} after \
                 {last:?}. Going backwards reads as an unanimated shape and leaves \
                 the registrations already stepped past consumed.",
                last = self.last_sampled,
            );
            self.last_sampled = Some(shape_idx);
        }
        let shape_idx = u64::from(shape_idx);
        while shape_idx > self.next_shape {
            self.advance();
        }
        // Lands on the first registration at or past `shape_idx`; only an exact hit
        // is this shape's. A jump past one registration must neither sample nor
        // consume the next. `CURSOR_END` (`u64::MAX`) matches no `u32` index.
        if shape_idx != self.next_shape {
            return PaintMod::IDENTITY;
        }
        let entry = self.entries[self.next];
        self.advance();
        entry.anim.sample(now)
    }

    #[inline]
    fn advance(&mut self) {
        self.next += 1;
        self.next_shape = next_shape(self.entries, self.next);
    }
}

#[cfg(test)]
mod tests;
