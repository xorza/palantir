use crate::internals::panic_probe;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::brush::gradient::Spread;
use crate::renderer::render_buffer::fill_form::{FillBrush, FillForm};
use crate::renderer::render_buffer::shape_form::ShapeForm;

#[test]
fn a_fill_takes_its_tags_brush() {
    for (kind, brush) in [
        (FillKind::SOLID, FillBrush::Solid),
        (FillKind::linear(Spread::Pad), FillBrush::Gradient),
        (FillKind::radial(Spread::Repeat), FillBrush::Gradient),
        (FillKind::conic(Spread::Reflect), FillBrush::Gradient),
    ] {
        assert_eq!(FillBrush::of(kind), brush, "{kind:?}");
    }
    panic_probe::assert_panics_with("a fill paints the tag", || FillBrush::of(FillKind::RAMP));
}

/// Each form's index is its place in `ALL`, so a form's pipeline slot is its own; a rect is no
/// fill's form.
#[test]
fn each_fill_form_indexes_its_place() {
    for (at, form) in FillForm::ALL.into_iter().enumerate() {
        assert_eq!(form.idx() as usize, at, "{form:?}");
        assert_eq!(FillForm::new(form.shape(), form.brush()), form);
    }
    assert_eq!(FillForm::BOX_SOLID.idx(), 0);
    panic_probe::assert_panics_with("a fill draws its shape's template", || {
        FillForm::new(ShapeForm::Rect, FillBrush::Solid)
    });
}
