use crate::internals::harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::align::{Align, HAlign, VAlign};
use crate::primitives::layout::sizing::Sizing;
use crate::scene::layer::Layer;
use crate::text::wrap::TextWrap;
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, panel::Panel, text::Text};
use glam::UVec2;

#[test]
fn canvas_places_child_at_position_within_inner_rect() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    let panel = h.under_outer(|ui| {
        Panel::canvas()
            .auto_id()
            .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
            .padding(10.0)
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("a"))
                    .position((30.0, 40.0))
                    .size((20.0, 20.0))
                    .show(ui);
            })
            .response
            .node()
    });
    let panel_rect = h.ui.arranged_rect(Layer::Main, panel);
    let kids: Vec<_> = h.main_child_rects(panel);
    let a = kids[0];
    assert_eq!(a.min.x - panel_rect.min.x, 40.0);
    assert_eq!(a.min.y, 50.0);
    assert_eq!(a.size.w, 20.0);
    assert_eq!(a.size.h, 20.0);
}

#[test]
fn canvas_hugs_to_bounding_box_of_placed_children() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    let panel = h.under_outer(|ui| {
        Panel::canvas()
            .auto_id()
            .size((Sizing::HUG, Sizing::HUG))
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("a"))
                    .position((10.0, 5.0))
                    .size((30.0, 15.0))
                    .show(ui);
                Block::new()
                    .id(WidgetId::from_hash("b"))
                    .position((50.0, 60.0))
                    .size((20.0, 20.0))
                    .show(ui);
            })
            .response
            .node()
    });
    let r = h.ui.arranged_rect(Layer::Main, panel);
    assert_eq!(r.size.w, 70.0);
    assert_eq!(r.size.h, 80.0);
}

/// Sister pin: a Fill canvas with a child positioned past its available width must not grow to wrap it, or it overflows its parent and the chrome paint rect shift flickers `Damage::Full` while dragging. Hug is unchanged.
#[test]
fn canvas_fill_canvas_positioned_overflow_does_not_grow_bbox() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    let panel = h.under_outer(|ui| {
        Panel::canvas()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("overhang"))
                    .position((700.0, 100.0))
                    .size((160.0, 80.0))
                    .show(ui);
            })
            .response
            .node()
    });
    let r = h.ui.arranged_rect(Layer::Main, panel);
    // Fill canvas in a 200×200 outer stays 200×200 regardless of the child's position.
    assert_eq!(
        r.size.w, 200.0,
        "FILL canvas width must not grow past available"
    );
    assert_eq!(r.size.h, 200.0);
    let kids: Vec<_> = h.main_child_rects(panel);
    let child = kids[0];
    // The child still arranges at its declared position and overflows.
    assert_eq!(child.min.x - r.min.x, 700.0);
    assert_eq!(child.min.y - r.min.y, 100.0);
}

#[test]
fn canvas_negative_position_does_not_extend_bbox() {
    // Canvas measures `max(pos + desired)` from zero, so negative coords bleed past the top-left instead of growing it.
    let mut h = UiHarness::new(UVec2::new(400, 400));
    let panel = h.under_outer(|ui| {
        Panel::canvas()
            .auto_id()
            .size((Sizing::HUG, Sizing::HUG))
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("neg"))
                    .position((-5.0, -5.0))
                    .size((20.0, 20.0))
                    .show(ui);
            })
            .response
            .node()
    });
    let r = h.ui.arranged_rect(Layer::Main, panel);
    assert_eq!(r.size.w, 15.0);
    assert_eq!(r.size.h, 15.0);

    let kids: Vec<_> = h.main_child_rects(panel);
    let child = kids[0];
    assert_eq!(child.min.x - r.min.x, -5.0);
    assert_eq!(child.min.y - r.min.y, -5.0);
}

/// A Fixed canvas gives a Fill child the room past its position (100 inner less 10 is 90); a Hug canvas passes INF, so Fill falls back to intrinsic. The position must come off the slot or the child overflows by the offset.
#[test]
fn canvas_fill_child_fills_the_room_past_its_position() {
    let cases: &[(&str, Option<f32>, f32)] = &[
        ("fixed_canvas_passes_the_room_left", Some(100.0), 90.0),
        ("hug_canvas_falls_back_to_intrinsic", None, 0.0),
    ];
    for (label, fixed_size, expected) in cases {
        let mut h = UiHarness::new(UVec2::new(400, 400));
        let panel = h.under_outer(|ui| {
            let mut canvas = Panel::canvas().auto_id();
            if let Some(s) = *fixed_size {
                canvas = canvas.size((Sizing::fixed(s), Sizing::fixed(s)));
            }
            canvas
                .show(ui, |ui| {
                    Block::new()
                        .id(WidgetId::from_hash("filler"))
                        .position((10.0, 10.0))
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui);
                })
                .response
                .node()
        });
        let kids: Vec<_> = h.main_child_rects(panel);
        let f = kids[0];
        assert_eq!(f.size.w, *expected, "case: {label} w");
        assert_eq!(f.size.h, *expected, "case: {label} h");
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
        let mut h = UiHarness::new(UVec2::new(400, 400));
        let panel = h.under_outer(|ui| {
            Panel::canvas()
                .auto_id()
                .size((Sizing::fixed(canvas_w), Sizing::fixed(200.0)))
                .show(ui, |ui| {
                    Text::new("aaaa bbbb cccc")
                        .id(WidgetId::from_hash("wrapping"))
                        .text_wrap(TextWrap::WrapWithOverflow)
                        .position((at_x, 0.0))
                        .show(ui);
                })
                .response
                .node()
        });
        h.main_child_rects(panel)[0].size
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

/// Pin: Canvas places children at their explicit `.position(...)` and ignores `.align(...)`, unlike Stack/ZStack/Grid.
#[test]
fn canvas_ignores_child_align() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    h.under_outer(|ui| {
        Panel::canvas()
            .auto_id()
            .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("aligned"))
                    .position((30.0, 40.0))
                    .size((50.0, 50.0))
                    // Would matter on Stack/ZStack/Grid; Canvas ignores it.
                    .align(Align::new(HAlign::Right, VAlign::Bottom))
                    .show(ui);
            });
    });
    let r = h.arranged(WidgetId::from_hash("aligned"));
    assert_eq!((r.min.x, r.min.y), (30.0, 40.0));
    assert_eq!((r.size.w, r.size.h), (50.0, 50.0));
}
