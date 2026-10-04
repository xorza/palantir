//! What one paint animation does to one shape at one instant.

/// Per-shape paint modification sampled from a `PaintAnimation`. Encoder
/// folds this into the shape's brush / geometry at emit time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PaintMod {
    /// Multiplies the shape's fill alpha. `1.0` = pass-through;
    /// `0.0` = fully transparent (encoder may drop the emit).
    pub(crate) alpha: f32,
    /// Rotation in radians applied to the shape's geometry about its
    /// owner-box centre at paint time. `0.0` = no rotation. Only a
    /// [`PaintChannel::turn`](crate::widget::PaintChannel) produces a non-zero
    /// value; the polyline, curve, and arc emits honour it (the composer rotates
    /// points / control points / center + angles before the ancestor
    /// transform). The encoder folds it and the pivot into the payload's
    /// [`StrokeBounds`](crate::renderer::frontend::payload::stroke_bounds::StrokeBounds).
    pub(crate) rotation: f32,
}

impl PaintMod {
    /// Pass-through sample. Returned by
    /// [`PaintAnimCursor::sample`](crate::scene::tree::paint_anims::PaintAnimCursor::sample)
    /// when a shape has no anim attached, so callers can fold the result
    /// unconditionally.
    pub(crate) const IDENTITY: Self = Self {
        alpha: 1.0,
        rotation: 0.0,
    };
}
