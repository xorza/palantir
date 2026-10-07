//! What one paint animation does to one shape at one instant.

/// Per-shape modification sampled from a `PaintAnimation`, folded into the shape at emit time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PaintMod {
    pub(crate) alpha: f32,
    /// Rotation in radians about the owner-box centre; only a [`PaintChannel::turn`](crate::widget::PaintChannel) makes it non-zero.
    pub(crate) rotation: f32,
}

impl PaintMod {
    pub(crate) const IDENTITY: Self = Self {
        alpha: 1.0,
        rotation: 0.0,
    };
}
