image

checkbox

combine wakes that happen almost at same time

for (layer, tree) in forest.iter_paint_order() { - iterating twice

PaintMod

    /// `docs/roadmap/paint-tick.md`.
    pub(crate) paint_anims: Vec<PaintAnimEntry>,

    /// One slot per `shapes.records[i]`. `PAINT_ANIM_NONE` for "no
    /// anim"; otherwise a (u16-sized) index into `paint_anims`.
    /// Encoder reads this at per-shape emit time; the niche keeps
    /// the no-anim hot path to a single load + branch.
    ///
    /// Grown in lockstep with `shapes.records` from
    /// `Forest::add_shape{,_animated}`. Cleared in `pre_record`.
    /// Capacity caps animated shapes per tree at `u16::MAX - 1`,
    /// which is well past anything realistic for paint animations
    /// (caret, spinner, pulse — order of dozens, not thousands).
    pub(crate) paint_anim_by_shape: Vec<u16>,
