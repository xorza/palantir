use crate::gpu::pipeline::quad_pipeline::quad_form::QuadForm;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::brush::gradient::Spread;

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
