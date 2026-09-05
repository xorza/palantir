//! Paint-only animations: the per-shape contract (`PaintAnim` /
//! `PaintMod`) and the per-tree registry that stores it.
//!
//! Paint anims are declarative shape-level animations that don't affect
//! layout, hit-test, or tree structure. Widgets register a `PaintAnim`
//! against a freshly-added shape via `Ui::add_shape_animated`; the
//! encoder samples it at paint time and folds the resulting [`PaintMod`]
//! (an alpha multiplier today; transform mod once the renderer can
//! express it) into the per-shape brush. `post_record` folds each anim's
//! `next_wake` into the `Ui` frame runtime's wake queue, so widget code never calls
//! `request_repaint_after` for these shapes.
//!
//! Unlike the value-interpolation animations in `crate::animation`
//! (record-time readback, keyed `(WidgetId, AnimSlot)`), paint anims are
//! sampled at *encode* time and stored on the `Tree`. They share no code
//! with that system — sampling is a pure function of `now`, with no
//! accumulator state, so dropped frames / irregular `dt` don't drift.
//!
//! The registry stores only live entries and their sorted shape indices.
//! Encoder traversal is monotonic in shape index, so a cursor advances
//! across both visited shapes and ranges skipped by subtree culling without
//! retaining a reverse-index slot for every preceding static shape.
//!
//! An animation drives alpha, rotation, or both. Translation and scale
//! would need the cascade to bound the *swept union over the path*, which
//! it cannot compute without sampling — where a rotation's cover is the
//! same square at every angle.

#[cfg(feature = "bench")]
pub(crate) mod bench;
pub mod curves;
pub(crate) mod paint_anim;

use crate::scene::tree::node_id::NodeId;
use crate::scene::tree::paint_anims::paint_anim::PaintAnim;
use std::time::Duration;

const CURSOR_END: u64 = u64::MAX;

/// Per-shape paint modification sampled from a `PaintAnim`. Encoder
/// folds this into the shape's brush / geometry at emit time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PaintMod {
    /// Multiplies the shape's fill alpha. `1.0` = pass-through;
    /// `0.0` = fully transparent (encoder may drop the emit).
    pub(crate) alpha: f32,
    /// Rotation in radians applied to the shape's geometry about its
    /// owner-box centre at paint time. `0.0` = no rotation. Only a
    /// [`PaintChannel::turn`](crate::PaintChannel) produces a non-zero
    /// value; the polyline, curve, and arc emits honour it (the composer rotates
    /// points / control points / center + angles before the ancestor
    /// transform). The encoder folds it and the pivot into the payload's
    /// [`StrokeBounds`](crate::renderer::frontend::payload::stroke_bounds::StrokeBounds).
    pub(crate) rotation: f32,
}

impl PaintMod {
    /// Pass-through sample. Returned by [`PaintAnimCursor::sample`] when a
    /// shape has no anim attached, so callers can fold the result
    /// unconditionally.
    pub(crate) const IDENTITY: Self = Self {
        alpha: 1.0,
        rotation: 0.0,
    };
}

/// One row per registered paint animation.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PaintAnimEntry {
    pub(crate) anim: PaintAnim,
    /// Index into `Tree::shapes.records` of the animated shape. Strictly
    /// increasing across [`PaintAnims::entries`], because registration
    /// follows append-only shape recording — which is what lets
    /// [`PaintAnims::rotates`] binary-search it and [`PaintAnimCursor`]
    /// walk it in one direction.
    pub(crate) shape_idx: u32,
    /// Paint-arena row of the animated shape inside its owner's
    /// `node_spans` span — the chrome offset plus the shape's position
    /// in the owner's `TreeItems` stream, captured from
    /// `OpenFrame::paint_rows` at `add_shape_animated` time. Lets
    /// damage's `extend_predamaged` index the shape's screen rect as
    /// `paint_arena.rows[node_span.start + row]` with no per-frame
    /// `TreeItems` walk.
    pub(crate) row: u32,
    /// The node that owns this shape — the open node at
    /// `add_shape_animated` time. Lets the damage lookup index
    /// `node_spans[node]` directly without needing a per-frame
    /// `shape_idx → paint_idx` reverse map.
    pub(crate) node: NodeId,
}

/// Per-tree sparse paint-animation registry, cleared per frame.
#[derive(Debug, Default)]
pub(crate) struct PaintAnims {
    /// Live anim entries, in registration order — which is shape order,
    /// so `shape_idx` increases down the column. Iterated by
    /// `Forest::min_paint_anim_wake` (next-wake fold) and
    /// `DamageEngine::compute` (anim-damage union), searched by
    /// [`Self::rotates`], and walked in step by [`PaintAnimCursor`].
    pub(crate) entries: Vec<PaintAnimEntry>,
}

impl PaintAnims {
    /// Reset for a fresh recording frame. Capacity retained — same
    /// lifecycle as every other per-frame tree column.
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

    /// Whether the shape at `shape_idx` paints under a rotation.
    ///
    /// The cascade's question, and it asks it without a `now`: what a
    /// rotating shape is culled and damaged against is the square it
    /// sweeps, which is the same at every angle. `entries` is ordered by
    /// `shape_idx` and holds only the handful of shapes a frame
    /// animates, so the search is over a list that is usually empty.
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
        .map_or(CURSOR_END, |entry| entry.shape_idx as u64)
}

/// Monotonic encoder lookup over the sparse animation rows.
#[derive(Debug)]
pub(crate) struct PaintAnimCursor<'a> {
    entries: &'a [PaintAnimEntry],
    next: usize,
    next_shape: u64,
    /// Previous [`Self::sample`] argument, so the monotonicity
    /// precondition is checked in debug rather than trusted.
    ///
    /// A backwards walk has no failure signal of its own: the cursor
    /// answers `IDENTITY`, which is also what an unanimated shape gets,
    /// and the registration it already stepped past stays consumed. The
    /// recording half asserts the same ordering in
    /// [`PaintAnims::push_entry`]; this is the reading half of it.
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
        let shape_idx = shape_idx as u64;
        while shape_idx > self.next_shape {
            self.advance();
        }
        // The loop lands on the first registration at or past `shape_idx`;
        // only an exact hit is this shape's. A jump that skips over one
        // registration and stops short of the next must not hand out the
        // next one's sample — and must not consume it either, or the
        // shape that owns it would then paint unanimated. `CURSOR_END`
        // is `u64::MAX`, which no `u32` index can equal, so an exhausted
        // cursor falls out here too.
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
