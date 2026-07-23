use crate::Ui;
use crate::layout::types::{align::Align, align::HAlign, align::VAlign, sizing::Sizing};
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::node::Configure;
use crate::widgets::{frame::Frame, panel::Panel};
use glam::UVec2;

#[test]
fn canvas_places_child_at_position_within_inner_rect() {
    let mut ui = Ui::for_test();
    let panel = ui.under_outer(UVec2::new(400, 400), |ui| {
        Panel::canvas()
            .auto_id()
            .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
            .padding(10.0)
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("a"))
                    .position((30.0, 40.0))
                    .size((20.0, 20.0))
                    .show(ui);
            })
            .response
            .node()
    });
    let panel_rect = ui.layout[Layer::Main].rect[panel.idx()];
    let kids: Vec<_> = ui.main_child_ids(panel);
    let a = ui.layout[Layer::Main].rect[kids[0].idx()];
    assert_eq!(a.min.x - panel_rect.min.x, 40.0);
    assert_eq!(a.min.y, 50.0);
    assert_eq!(a.size.w, 20.0);
    assert_eq!(a.size.h, 20.0);
}

#[test]
fn canvas_hugs_to_bounding_box_of_placed_children() {
    let mut ui = Ui::for_test();
    let panel = ui.under_outer(UVec2::new(400, 400), |ui| {
        Panel::canvas()
            .auto_id()
            .size((Sizing::HUG, Sizing::HUG))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("a"))
                    .position((10.0, 5.0))
                    .size((30.0, 15.0))
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("b"))
                    .position((50.0, 60.0))
                    .size((20.0, 20.0))
                    .show(ui);
            })
            .response
            .node()
    });
    let r = ui.layout[Layer::Main].rect[panel.idx()];
    // bbox = max(pos + desired) per axis: 50+20=70, 60+20=80
    assert_eq!(r.size.w, 70.0);
    assert_eq!(r.size.h, 80.0);
}

/// Sister pin to `canvas_negative_position_does_not_extend_bbox`: a FILL
/// canvas with a child positioned past its available width does NOT
/// grow to wrap the child. Without this gate, `Fill = available floored
/// at intrinsic_min` floors above `available` and the canvas overflows
/// its parent — and the resulting per-frame chrome paint rect shift drives
/// `Damage::Full` flicker on every drag-the-node-past-the-edge tick
/// (the darkroom graph-view bug). Hug canvas behavior is unchanged
/// (verified by `canvas_places_child_at_position_within_inner_rect` and
/// `canvas_two_children_take_bbox_max_position_plus_size`).
#[test]
fn canvas_fill_canvas_positioned_overflow_does_not_grow_bbox() {
    let mut ui = Ui::for_test();
    let panel = ui.under_outer(UVec2::new(200, 200), |ui| {
        Panel::canvas()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("overhang"))
                    .position((700.0, 100.0))
                    .size((160.0, 80.0))
                    .show(ui);
            })
            .response
            .node()
    });
    let r = ui.layout[Layer::Main].rect[panel.idx()];
    // FILL canvas in a 200×200 outer: stays at 200×200 regardless of
    // the child's position. Pre-fix this was 860×200 (700 + 160).
    assert_eq!(
        r.size.w, 200.0,
        "FILL canvas width must not grow past available"
    );
    assert_eq!(r.size.h, 200.0);
    let kids: Vec<_> = ui.main_child_ids(panel);
    let child = ui.layout[Layer::Main].rect[kids[0].idx()];
    // Child still arranges at its declared position — it just overflows
    // the canvas (and would be clipped by any ancestor with
    // `.clip_rect()`).
    assert_eq!(child.min.x - r.min.x, 700.0);
    assert_eq!(child.min.y - r.min.y, 100.0);
}

#[test]
fn canvas_negative_position_does_not_extend_bbox() {
    // Canvas measures `max(pos + desired)` starting at zero, so children
    // placed at negative coords don't grow the panel — they just bleed past
    // the inner top-left. Scrollable negative-origin canvases are a
    // userspace concern via `Scroll::anchor_canvas_origin`.
    let mut ui = Ui::for_test();
    let panel = ui.under_outer(UVec2::new(400, 400), |ui| {
        Panel::canvas()
            .auto_id()
            .size((Sizing::HUG, Sizing::HUG))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("neg"))
                    .position((-5.0, -5.0))
                    .size((20.0, 20.0))
                    .show(ui);
            })
            .response
            .node()
    });
    let r = ui.layout[Layer::Main].rect[panel.idx()];
    // pos + desired = (15, 15) per axis.
    assert_eq!(r.size.w, 15.0);
    assert_eq!(r.size.h, 15.0);

    let kids: Vec<_> = ui.main_child_ids(panel);
    let child = ui.layout[Layer::Main].rect[kids[0].idx()];
    assert_eq!(child.min.x - r.min.x, -5.0);
    assert_eq!(child.min.y - r.min.y, -5.0);
}

/// A constrained Canvas (Fixed) passes its inner to children, so a Fill
/// child takes the canvas's full inner. A Hug Canvas passes INF on Hug
/// axes → Fill falls back to intrinsic (zero for an empty Frame). This
/// preserves the recursive-sizing protection for Hug parents.
///
/// The old "Fill = 0 in Canvas" rule (Fill always intrinsic) was a
/// Canvas-specific quirk that broke constraint propagation for Hug Grid
/// children. Authors who genuinely want "no Fill behavior" can use
/// `Sizing::HUG`.
#[test]
fn canvas_fill_child_uses_inner_when_constrained_else_intrinsic() {
    let cases: &[(&str, Option<f32>, f32)] = &[
        ("fixed_canvas_passes_inner", Some(100.0), 100.0),
        ("hug_canvas_falls_back_to_intrinsic", None, 0.0),
    ];
    for (label, fixed_size, expected) in cases {
        let mut ui = Ui::for_test();
        let panel = ui.under_outer(UVec2::new(400, 400), |ui| {
            let mut canvas = Panel::canvas().auto_id();
            if let Some(s) = *fixed_size {
                canvas = canvas.size((Sizing::fixed(s), Sizing::fixed(s)));
            }
            canvas
                .show(ui, |ui| {
                    Frame::new()
                        .id(WidgetId::from_hash("filler"))
                        .position((10.0, 10.0))
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui);
                })
                .response
                .node()
        });
        let kids: Vec<_> = ui.main_child_ids(panel);
        let f = ui.layout[Layer::Main].rect[kids[0].idx()];
        assert_eq!(f.size.w, *expected, "case: {label} w");
        assert_eq!(f.size.h, *expected, "case: {label} h");
    }
}

#[test]
fn canvas_collapsed_child_does_not_grow_bbox() {
    let mut ui = Ui::for_test();
    let panel = ui.under_outer(UVec2::new(400, 400), |ui| {
        Panel::canvas()
            .auto_id()
            .size((Sizing::HUG, Sizing::HUG))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("a"))
                    .position((0.0, 0.0))
                    .size((10.0, 10.0))
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("collapsed"))
                    .position((100.0, 100.0))
                    .size((50.0, 50.0))
                    .collapsed()
                    .show(ui);
            })
            .response
            .node()
    });
    let r = ui.layout[Layer::Main].rect[panel.idx()];
    assert_eq!(r.size.w, 10.0);
    assert_eq!(r.size.h, 10.0);
}

/// Pin: Canvas places children at their explicit `.position(...)` and
/// **ignores `.align(...)`** — children's alignment values do not
/// participate in placement (Canvas is the "explicit position wins"
/// driver). Stack/ZStack/Grid all consume align via shared axis resolution;
/// Canvas does not. Adding align-cascade to Canvas would seem like a
/// reasonable change but would break the contract that Canvas users
/// rely on for free-form placement.
#[test]
fn canvas_ignores_child_align() {
    let mut ui = Ui::for_test();
    let mut child = None;
    let _panel = ui.under_outer(UVec2::new(400, 400), |ui| {
        Panel::canvas()
            .auto_id()
            .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
            .show(ui, |ui| {
                child = Some(
                    Frame::new()
                        .id(WidgetId::from_hash("aligned"))
                        .position((30.0, 40.0))
                        .size((50.0, 50.0))
                        // Right/Bottom would matter on Stack/ZStack/Grid;
                        // Canvas must ignore it.
                        .align(Align::new(HAlign::Right, VAlign::Bottom))
                        .show(ui)
                        .node(),
                );
            })
            .response
            .node()
    });
    let r = ui.layout[Layer::Main].rect[child.unwrap().idx()];
    assert_eq!((r.min.x, r.min.y), (30.0, 40.0));
    assert_eq!((r.size.w, r.size.h), (50.0, 50.0));
}
