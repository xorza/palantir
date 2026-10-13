//! Which specialization of `fs` a quad draws through.

use crate::primitives::packed::fill_kind::FillKind;

/// The paths of `fs` a quad reaches, pinned per pipeline as `QUAD_FORM`, so a solid rect skips a gradient's register cost (Pi 5 V3D: 54 temporaries and two threads for all forms, 22 and four for solid).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QuadForm {
    /// Rounded rect, solid fill.
    Solid = 0,
    /// Rounded rect, gradient fill.
    Gradient = 1,
    /// Rounded triangle.
    Triangle = 2,
}

impl QuadForm {
    /// The form a quad of `kind` draws through; an unknown tag takes the gradient form, whose `eval_fill` falls back to solid.
    pub(crate) const fn of(kind: FillKind) -> Self {
        match kind.tag() {
            FillKind::TAG_SOLID => Self::Solid,
            FillKind::TAG_TRIANGLE => Self::Triangle,
            _ => Self::Gradient,
        }
    }
}

#[cfg(test)]
mod tests;
