use crate::internals::panic_probe;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::brush::gradient::Spread;
use crate::renderer::render_buffer::quad_form::QuadForm;

#[test]
fn a_quad_takes_its_tags_form() {
    for (kind, form) in [
        (FillKind::SOLID, QuadForm::BoxSolid),
        (FillKind::SOLID.with_fast(), QuadForm::BoxSolid),
        (FillKind::SOLID.with_outside(), QuadForm::BoxOutside),
        (FillKind::linear(Spread::Pad), QuadForm::BoxGradient),
        (
            FillKind::radial(Spread::Repeat).with_outside(),
            QuadForm::BoxOutside,
        ),
        (FillKind::conic(Spread::Reflect), QuadForm::BoxGradient),
        (FillKind::SOLID.with_triangle(), QuadForm::TriangleSolid),
        (
            FillKind::conic(Spread::Pad).with_triangle(),
            QuadForm::TriangleGradient,
        ),
    ] {
        assert_eq!(QuadForm::of(kind), form, "{kind:?}");
    }
}

/// No quad form draws a shadow or the curves' ramp.
#[test]
fn a_tag_no_form_draws_panics() {
    for kind in [
        FillKind::SHADOW_DROP,
        FillKind::SHADOW_INSET,
        FillKind::RAMP,
        FillKind::RAMP.with_triangle(),
    ] {
        panic_probe::assert_panics_with("a quad form draws the tag", || QuadForm::of(kind));
    }
}

/// A triangle's forms paint inside it alone.
#[test]
fn a_triangle_painted_outside_panics() {
    for kind in [FillKind::SOLID, FillKind::radial(Spread::Pad)] {
        panic_probe::assert_panics_with("a triangle paints inside its shape", || {
            QuadForm::of(kind.with_triangle().with_outside())
        });
    }
}
