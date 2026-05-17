//! Per-tree paint-animation registry. Pairs the list of live anim
//! entries with a shape-indexed lookup table so the encoder can map
//! a `(shape_idx) → PaintMod` in one indexed load + branch on the hot
//! path. `by_shape` is **lazy**: empty when no shape this frame is
//! animated, and only grown out to `shape_idx + 1` on the first
//! `push_entry` call. Encoder treats `shape_idx >= by_shape.len()`
//! as "no anim" so the no-anim path costs one length compare, and
//! `Forest::add_shape` doesn't push a sentinel per shape in the
//! common (no-anim) frame.
//!
//! See `docs/roadmap/paint-tick.md` for the full design.

use crate::animation::paint::{PaintAnim, PaintMod};
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
    /// Read by `Ui::predamaged_rects` to look up the shape's tight
    /// screen-space damage rect off `Cascades::shape_rects[layer]`.
    /// Encoder uses the parallel `by_shape` array instead.
    pub(crate) shape_idx: u32,
}

/// Per-tree paint-animation registry. Pushed in lockstep with the
/// shape buffer; cleared per frame.
#[derive(Default)]
pub(crate) struct PaintAnims {
    /// Live anim entries, in registration order. Iterated by
    /// `Tree::post_record` (quantum + next_wake fold) and
    /// `DamageEngine::compute` (anim-damage union).
    pub(crate) entries: Vec<PaintAnimEntry>,
    /// Sparse `shape_idx → entries[idx]` lookup. Empty when no shape
    /// this frame is animated (the common case). Grown only when the
    /// first animated shape arrives — padded out to `shape_idx + 1`
    /// with [`PAINT_ANIM_NONE`] and the animated slot stamped. Encoder
    /// treats `shape_idx >= by_shape.len()` as "no anim", so unanimated
    /// shapes pay zero `Vec::push` per frame.
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

    /// Register `entry` against the just-pushed shape. Lazily grows
    /// `by_shape` to `entry.shape_idx + 1`, padding any preceding
    /// (unanimated) shapes with [`PAINT_ANIM_NONE`]. Asserts the
    /// `entries` cap so a `u16` index always fits in `by_shape`.
    pub(crate) fn push_entry(&mut self, entry: PaintAnimEntry) {
        let idx = self.entries.len();
        assert!(
            idx < PAINT_ANIM_NONE as usize,
            "more than {PAINT_ANIM_NONE} paint-anim entries in one tree — bump by_shape to u32",
        );
        let shape_idx = entry.shape_idx as usize;
        if self.by_shape.len() <= shape_idx {
            self.by_shape.resize(shape_idx + 1, PAINT_ANIM_NONE);
        }
        self.by_shape[shape_idx] = idx as u16;
        self.entries.push(entry);
    }

    /// Sample the anim attached to shape `shape_idx`, if any. Returns
    /// [`PaintMod::IDENTITY`] on the hot path (no anim — the vast
    /// majority of shapes), so callers can fold the result
    /// unconditionally once we ship variants beyond binary blink.
    #[inline]
    pub(crate) fn sample(&self, shape_idx: u32, now: Duration) -> PaintMod {
        let Some(&slot) = self.by_shape.get(shape_idx as usize) else {
            return PaintMod::IDENTITY;
        };
        if slot == PAINT_ANIM_NONE {
            return PaintMod::IDENTITY;
        }
        self.entries[slot as usize].anim.sample(now)
    }
}
