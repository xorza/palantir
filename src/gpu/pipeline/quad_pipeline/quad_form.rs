//! Which specialization of `fs` a quad draws through.

use crate::primitives::packed::fill_kind::FillKind;

/// The paths of `fs` a quad reaches, pinned per pipeline as `QUAD_FORM`.
/// The driver drops the paths a form never takes, so a solid rect does not
/// pay the registers of a gradient or a triangle: on the Pi 5's V3D, `fs`
/// for every form at once holds 54 temporaries and runs two threads, and
/// the solid form 22 and four.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QuadForm {
    /// A rounded rect with a solid fill, stroked or windowed.
    Solid = 0,
    /// A rounded rect filled by a gradient, stroked or windowed.
    Gradient = 1,
    /// A rounded triangle.
    Triangle = 2,
}

impl QuadForm {
    /// The form a quad of `kind` draws through. A tag `fs` does not know
    /// takes the gradient form, whose `eval_fill` falls back to the solid
    /// fill for it.
    pub(crate) const fn of(kind: FillKind) -> Self {
        match kind.tag() {
            FillKind::TAG_SOLID => Self::Solid,
            FillKind::TAG_TRIANGLE => Self::Triangle,
            _ => Self::Gradient,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::pipeline::quad_pipeline::quad_form::QuadForm;
    use crate::primitives::packed::fill_kind::FillKind;
    use crate::primitives::paint::brush::gradient::Spread;

    /// The flags beside the tag do not move a quad between forms.
    #[test]
    fn a_quad_takes_its_tags_form() {
        for (kind, form) in [
            (FillKind::SOLID, QuadForm::Solid),
            (FillKind::SOLID.with_fast(), QuadForm::Solid),
            (FillKind::SOLID.with_window(), QuadForm::Solid),
            (FillKind::linear(Spread::Pad), QuadForm::Gradient),
            (
                FillKind::radial(Spread::Repeat).with_window(),
                QuadForm::Gradient,
            ),
            (FillKind::conic(Spread::Reflect), QuadForm::Gradient),
            (FillKind::TRIANGLE, QuadForm::Triangle),
        ] {
            assert_eq!(QuadForm::of(kind), form, "{kind:?}");
        }
    }
}
