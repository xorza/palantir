use crate::internals::layout_rig::{LayoutRig, TextRun};
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::align::{HAlign, VAlign};
use crate::primitives::layout::sizing::{SizeSpec, Sizing};
use crate::text::wrap::TextWrap;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;

const SURFACE: Size = Size::new(400.0, 400.0);

#[test]
fn canvas_places_child_at_position_within_inner_rect() {
    let mut rig = LayoutRig::new();
    rig.run(SURFACE, |rec| {
        rec.node(
            "panel",
            Widget::canvas()
                .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                .padding(10.0),
            |rec| rec.leaf("a", Widget::leaf().at((30.0, 40.0)).size((20.0, 20.0))),
        );
    });
    assert_eq!(rig.rect("a"), Rect::new(40.0, 50.0, 20.0, 20.0));
}

#[test]
fn canvas_hugs_to_bounding_box_of_placed_children() {
    let mut rig = LayoutRig::new();
    rig.run(SURFACE, |rec| {
        rec.node("panel", Widget::canvas(), |rec| {
            rec.leaf("a", Widget::leaf().at((10.0, 5.0)).size((30.0, 15.0)));
            rec.leaf("b", Widget::leaf().at((50.0, 60.0)).size((20.0, 20.0)));
        });
    });
    assert_eq!(rig.rect("panel").size, Size::new(70.0, 80.0));
}

/// Sister pin: a Fill canvas with a child positioned past its available width must not grow to wrap it, or it overflows its parent and the chrome paint rect shift flickers `Damage::Full` while dragging. Hug is unchanged.
#[test]
fn canvas_fill_canvas_positioned_overflow_does_not_grow_bbox() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(200.0, 200.0), |rec| {
        rec.node("panel", Widget::canvas().size(SizeSpec::FILL), |rec| {
            rec.leaf(
                "overhang",
                Widget::leaf().at((700.0, 100.0)).size((160.0, 80.0)),
            );
        });
    });
    // Fill canvas on a 200×200 surface stays 200×200 regardless of the child's position.
    assert_eq!(
        rig.rect("panel"),
        Rect::new(0.0, 0.0, 200.0, 200.0),
        "FILL canvas must not grow past available"
    );
    // The child still arranges at its declared position and overflows.
    assert_eq!(rig.rect("overhang"), Rect::new(700.0, 100.0, 160.0, 80.0));
}

/// A Fill canvas below a 50-tall header in a 200-tall column gets the 150 left, though its child is
/// 300 tall: a bounded axis has no floor, so the child overflows instead of pushing the column past
/// the window, as a floor of the child's height would, to 350.
#[test]
fn canvas_fill_axis_has_no_floor() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(200.0, 200.0), |rec| {
        rec.node("column", Widget::vstack().size(SizeSpec::FILL), |rec| {
            rec.leaf(
                "header",
                Widget::leaf().size((Sizing::FILL, Sizing::fixed(50.0))),
            );
            rec.node("canvas", Widget::canvas().size(SizeSpec::FILL), |rec| {
                rec.leaf("tall", Widget::leaf().at((10.0, 20.0)).size((50.0, 300.0)));
            });
        });
    });
    assert_eq!(rig.rect("column").size.h, 200.0);
    let canvas = rig.rect("canvas");
    assert_eq!((canvas.min.y, canvas.size.h), (50.0, 150.0));
    let tall = rig.rect("tall");
    assert_eq!((tall.min.y, tall.size.h), (70.0, 300.0));
}

#[test]
fn canvas_negative_position_does_not_extend_bbox() {
    // Canvas measures `max(pos + desired)` from zero, so negative coords bleed past the top-left instead of growing it.
    let mut rig = LayoutRig::new();
    rig.run(SURFACE, |rec| {
        rec.node("panel", Widget::canvas(), |rec| {
            rec.leaf("neg", Widget::leaf().at((-5.0, -5.0)).size((20.0, 20.0)));
        });
    });
    assert_eq!(rig.rect("panel").size, Size::new(15.0, 15.0));
    assert_eq!(rig.rect("neg"), Rect::new(-5.0, -5.0, 20.0, 20.0));
}

/// A Fixed canvas gives a Fill child the room past its position (100 inner less 10 is 90); a Hug canvas passes INF, so Fill falls back to intrinsic. The position must come off the slot or the child overflows by the offset.
#[test]
fn canvas_fill_child_fills_the_room_past_its_position() {
    let cases: &[(&str, Option<f32>, f32)] = &[
        ("fixed_canvas_passes_the_room_left", Some(100.0), 90.0),
        ("hug_canvas_falls_back_to_intrinsic", None, 0.0),
    ];
    for (label, fixed_size, expected) in cases {
        let mut rig = LayoutRig::new();
        rig.run(SURFACE, |rec| {
            let mut canvas = Widget::canvas();
            if let Some(s) = *fixed_size {
                canvas = canvas.size((Sizing::fixed(s), Sizing::fixed(s)));
            }
            rec.node("panel", canvas, |rec| {
                rec.leaf(
                    "filler",
                    Widget::leaf().at((10.0, 10.0)).size(SizeSpec::FILL),
                );
            });
        });
        let f = rig.rect("filler");
        assert_eq!(f.size, Size::new(*expected, *expected), "case: {label}");
        if let Some(size) = *fixed_size {
            assert_eq!(
                f.size.w + 10.0,
                size,
                "case: {label} — the child ends on the canvas's inner edge",
            );
        }
    }
}

/// Pin: a bounded canvas measures a child against the room it will arrange into. A wrapping child at x = 100 in a 200-wide canvas must match the same child at x = 0 in a 100-wide one; x = 0 in the 200-wide canvas is the control (fits on one line).
#[test]
fn canvas_measures_a_child_against_the_room_past_its_position() {
    let sized = |canvas_w: f32, at_x: f32| {
        let mut rig = LayoutRig::new();
        rig.run(SURFACE, |rec| {
            rec.node(
                "panel",
                Widget::canvas().size((Sizing::fixed(canvas_w), Sizing::fixed(200.0))),
                |rec| {
                    rec.text_leaf(
                        "wrapping",
                        Widget::leaf().at((at_x, 0.0)),
                        TextRun::new("aaaa bbbb cccc", 16.0, TextWrap::WrapWithOverflow),
                    );
                },
            );
        });
        rig.rect("wrapping").size
    };
    let offset_in_wide = sized(200.0, 100.0);
    assert_eq!(
        offset_in_wide,
        sized(100.0, 0.0),
        "the room past x = 100 in a 200-wide canvas is a 100-wide canvas",
    );
    assert_ne!(
        offset_in_wide,
        sized(200.0, 0.0),
        "the same text unwrapped is a different size, or this proves nothing",
    );
}

/// Pin: Canvas places children at their explicit `.at(...)` and ignores `.align(...)`, unlike Stack/ZStack/Grid.
#[test]
fn canvas_ignores_child_align() {
    let mut rig = LayoutRig::new();
    rig.run(SURFACE, |rec| {
        rec.node(
            "panel",
            Widget::canvas().size((Sizing::fixed(200.0), Sizing::fixed(200.0))),
            |rec| {
                rec.leaf(
                    "aligned",
                    Widget::leaf()
                        .at((30.0, 40.0))
                        .size((50.0, 50.0))
                        // Would matter on Stack/ZStack/Grid; Canvas ignores it.
                        .align((HAlign::Right, VAlign::Bottom)),
                );
            },
        );
    });
    assert_eq!(rig.rect("aligned"), Rect::new(30.0, 40.0, 50.0, 50.0));
}
