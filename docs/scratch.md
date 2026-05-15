image

checkbox

combine wakes that happen almost at same time

let mut min_wake = Duration::MAX;
for entry in &mut self.paint_anims {
let q = entry.anim.quantum(now);
entry.last_quantum = q;
let w = entry.anim.next_wake(now);
if w < min_wake {
min_wake = w;
}

            // Fold the quantum into the owning node's hashes so
            // `DamageEngine` sees a phase flip as a paint-changed
            // event. Without this the shape buffer is bit-identical
            // across blink phases (the encoder hides the rect via
            // `paint_anim_hides`), so subtree-skip would miss the
            // damage. XOR mix is associative + commutative — multiple
            // anims under one node fold cleanly, and nested ancestors
            // accumulate without ordering hazards.
            let mix = (q as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
            let owner = entry.node;
            self.rollups.node[owner.index()].0 ^= mix;
            let mut cur = owner;
            while cur != NodeId::ROOT {
                self.rollups.subtree[cur.index()].0 ^= mix;
                cur = self.parents[cur.index()];
            }

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
