//! Which specialization of the quad fragment stage a quad draws through.

use crate::primitives::packed::fill_kind::FillKind;

/// The paths of the fragment stage (`quad_paint`, `fs_template`) a quad reaches, pinned per pipeline as `QUAD_FORM`, so a solid rect skips a gradient's register cost (VideoCore: 54 temporaries and two threads for all forms, 22 and four for solid).
///
/// Named shape first, then paint: a box, a rect's or a circle's, or a triangle, under a solid or
/// a gradient fill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QuadForm {
    /// A box, solid fill.
    BoxSolid = 0,
    /// A box, gradient fill.
    BoxGradient = 1,
    /// A box that paints outside its shape ([`FillKind::OUTSIDE_BIT`]), a solid or a gradient
    /// fill: its own form, so its rect-edge fade and its gradient reads stay out of the solid
    /// form's fragments, whose cost on an Apple GPU follows the varyings they read.
    BoxOutside = 2,
    /// A triangle, solid fill.
    TriangleSolid = 3,
    /// A triangle, gradient fill.
    TriangleGradient = 4,
}

impl QuadForm {
    /// The form a quad of `kind` draws through.
    ///
    /// # Panics
    ///
    /// Panics on a tag no quad form draws: a shadow, which draws through the shadow pipelines,
    /// or the curves' ramp. Panics on a triangle painted outside its shape.
    pub(crate) const fn of(kind: FillKind) -> Self {
        if kind.is_triangle() {
            assert!(!kind.is_outside(), "a triangle paints inside its shape");
            return match kind.tag() {
                FillKind::TAG_SOLID => Self::TriangleSolid,
                FillKind::TAG_LINEAR | FillKind::TAG_RADIAL | FillKind::TAG_CONIC => {
                    Self::TriangleGradient
                }
                _ => panic!("a quad form draws the tag"),
            };
        }
        match kind.tag() {
            FillKind::TAG_SOLID
            | FillKind::TAG_LINEAR
            | FillKind::TAG_RADIAL
            | FillKind::TAG_CONIC
                if kind.is_outside() =>
            {
                Self::BoxOutside
            }
            FillKind::TAG_SOLID => Self::BoxSolid,
            FillKind::TAG_LINEAR | FillKind::TAG_RADIAL | FillKind::TAG_CONIC => Self::BoxGradient,
            _ => panic!("a quad form draws the tag"),
        }
    }
}

#[cfg(test)]
mod tests;
