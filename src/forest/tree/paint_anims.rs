//! Per-tree paint-animation registry. Pairs the list of live anim
//! entries with a shape-indexed lookup table so the encoder can map
//! a `(shape_idx) → PaintMod` in one indexed load + branch on the hot
//! path. Cleared and grown in lockstep with `Tree::shapes.records`
//! via the dedicated `push_*` methods — drifting the two halves apart
//! would silently mis-index at encode time, so direct field
//! mutation outside this module is gated by the lockstep methods.
//!
//! See `docs/roadmap/paint-tick.md` for the full design.

use crate::animation::paint::{PaintAnim, PaintMod};
use crate::forest::tree::NodeId;
use std::time::Duration;

/// Sentinel in [`PaintAnims::by_shape`] meaning "this shape has no
/// paint-anim registration". `u16::MAX` mirrors the niche convention
/// used by `Slot::ABSENT` for the sparse extras tables — keeps the
/// encoder's per-shape lookup a single load + cmp.
const PAINT_ANIM_NONE: u16 = u16::MAX;

/// One row per registered paint animation. Lives in
/// [`PaintAnims::entries`], indexed by `by_shape[shape_idx]` (which
/// holds [`PAINT_ANIM_NONE`] when the shape isn't animated).
#[derive(Clone, Copy, Debug)]
pub(crate) struct PaintAnimEntry {
    pub(crate) anim: PaintAnim,
    /// Index into `Tree::shapes.records` of the shape this anim drives.
    /// Used by the future damage-region computation (slice 2) to look
    /// up the owning node via the per-node shape spans; encoder reads
    /// the entry through the parallel `by_shape` array instead and
    /// doesn't need this back-reference.
    #[allow(dead_code)] // consumed by slice-2 damage walk
    pub(crate) shape_idx: u32,
    /// Node that owns the animated shape — set in
    /// `Forest::add_shape_animated` from the currently-open frame.
    /// Read by `DamageEngine::compute` to look up `paint_rect` for
    /// the anim-damage union.
    pub(crate) node: NodeId,
    /// `anim.quantum(now)` captured in `Tree::post_record`. Drives
    /// slice-2's short-circuit damage walk: compares against
    /// `quantum(now_next)` on the next frame to detect a flip without
    /// rehashing.
    pub(crate) last_quantum: i32,
}

/// Per-tree paint-animation registry. Pushed in lockstep with the
/// shape buffer; cleared per frame.
#[derive(Default)]
pub(crate) struct PaintAnims {
    /// Live anim entries, in registration order. Iterated by
    /// `Tree::post_record` (quantum + next_wake fold) and
    /// `DamageEngine::compute` (anim-damage union).
    pub(crate) entries: Vec<PaintAnimEntry>,
    /// One slot per `Tree::shapes.records[i]`. [`PAINT_ANIM_NONE`] for
    /// "no anim"; otherwise a `u16` index into `entries`. Encoder
    /// reads this at per-shape emit time; the niche keeps the
    /// no-anim hot path to a single load + branch.
    ///
    /// Capacity caps animated shapes per tree at `u16::MAX - 1`,
    /// which is well past anything realistic (caret, spinner, pulse
    /// — order of dozens, not thousands).
    pub(crate) by_shape: Vec<u16>,
}

impl PaintAnims {
    /// Reset both columns for a fresh recording frame. Capacity
    /// retained — same lifecycle as every other per-frame tree
    /// column.
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.by_shape.clear();
    }

    /// Record that a non-animated shape was pushed to
    /// `Tree::shapes.records`. Keeps `by_shape.len() ==
    /// shapes.records.len()` so the encoder's index lookup is sound.
    pub(crate) fn push_unanimated(&mut self) {
        self.by_shape.push(PAINT_ANIM_NONE);
    }

    /// Register `entry` against the just-pushed shape. Asserts the
    /// `entries` cap so a `u16` index always fits in `by_shape`.
    pub(crate) fn push_entry(&mut self, entry: PaintAnimEntry) {
        let idx = self.entries.len();
        assert!(
            idx < PAINT_ANIM_NONE as usize,
            "more than {PAINT_ANIM_NONE} paint-anim entries in one tree — bump by_shape to u32",
        );
        self.entries.push(entry);
        self.by_shape.push(idx as u16);
    }

    /// True if at least one entry's next-wake fell in `(prev_now, now]`
    /// — i.e. the visible state actually flipped since last frame.
    /// Used by `Ui::frame_inner`'s short-circuit gate to confirm the
    /// frame fired *because* of a paint anim, not despite one.
    pub(crate) fn any_fired(&self, prev_now: Duration, now: Duration) -> bool {
        self.entries
            .iter()
            .any(|e| e.anim.next_wake(prev_now) <= now)
    }

    /// Sample the anim attached to shape `shape_idx`, if any. Returns
    /// [`PaintMod::IDENTITY`] on the hot path (no anim — the vast
    /// majority of shapes), so callers can fold the result
    /// unconditionally once we ship variants beyond binary blink.
    #[inline]
    pub(crate) fn sample(&self, shape_idx: u32, now: Duration) -> PaintMod {
        let slot = self.by_shape[shape_idx as usize];
        if slot == PAINT_ANIM_NONE {
            return PaintMod::IDENTITY;
        }
        self.entries[slot as usize].anim.sample(now)
    }
}
