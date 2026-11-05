//! Which specialization of `fs` a quad draws through.

use crate::primitives::packed::fill_kind::FillKind;

/// The paths of `fs` a quad reaches, pinned per pipeline as `QUAD_FORM`, so a solid rect skips a gradient's register cost (VideoCore: 54 temporaries and two threads for all forms, 22 and four for solid).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QuadForm {
    /// Rounded rect, solid fill.
    Solid = 0,
    /// Rounded rect, gradient fill.
    Gradient = 1,
    /// Rounded triangle.
    Triangle = 2,
    /// A pane over its backdrop, which reads the frame's backdrop results at group 2.
    Backdrop = 3,
    /// [`Self::Backdrop`] of a pane of glass of one index.
    BackdropLens = 4,
    /// [`Self::Backdrop`] of a pane of glass that spreads the spectrum.
    BackdropSpectrum = 5,
}

impl QuadForm {
    /// The form a quad of `kind` draws through.
    ///
    /// # Panics
    ///
    /// Panics on a tag no quad form draws: a shadow, which draws through the shadow pipelines,
    /// or the curves' ramp.
    pub(crate) const fn of(kind: FillKind) -> Self {
        match kind.tag() {
            FillKind::TAG_SOLID => Self::Solid,
            FillKind::TAG_LINEAR | FillKind::TAG_RADIAL | FillKind::TAG_CONIC => Self::Gradient,
            FillKind::TAG_TRIANGLE => Self::Triangle,
            FillKind::TAG_BACKDROP => Self::Backdrop,
            FillKind::TAG_BACKDROP_LENS => Self::BackdropLens,
            FillKind::TAG_BACKDROP_SPECTRUM => Self::BackdropSpectrum,
            _ => panic!("a quad form draws the tag"),
        }
    }
}

#[cfg(test)]
mod tests;
