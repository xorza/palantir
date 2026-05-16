use crate::forest::element::Configure;
use crate::forest::tree::Layer;
use crate::layout::types::{align::Align, align::HAlign, align::VAlign, sizing::Sizing};
use crate::ui::test_support::new_ui;
use crate::widgets::{frame::Frame, panel::Panel};
use glam::UVec2;

#[test]
fn canvas_places_child_at_position_within_inner_rect() {
    let mut ui = new_ui();
    let panel = ui.under_outer(UVec2::new(400, 400), |ui| {
        Panel::canvas()
            .auto_id()
            .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
            .padding(10.0)
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("a")
                    .position((30.0, 40.0))
                    .size((20.0, 20.0))
                    .show(ui);
            })
            .node(ui)
    });
    let panel_rect = ui.layout[Layer::Main].rect[panel.index()];
    let kids: Vec<_> = ui
        .forest
        .tree(Layer::Main)
        .children(panel)
        .map(|c| c.id)
        .collect();
    let a = ui.layout[Layer::Main].rect[kids[0].index()];
    assert_eq!(a.min.x - panel_rect.min.x, 40.0);
    assert_eq!(a.min.y, 50.0);
    assert_eq!(a.size.w, 20.0);
    assert_eq!(a.size.h, 20.0);
}

#[test]
fn canvas_hugs_to_bounding_box_of_placed_children() {
    let mut ui = new_ui();
    let panel = ui.under_outer(UVec2::new(400, 400), |ui| {
        Panel::canvas()
            .auto_id()
            .size((Sizing::Hug, Sizing::Hug))
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("a")
                    .position((10.0, 5.0))
                    .size((30.0, 15.0))
                    .show(ui);
                Frame::new()
                    .id_salt("b")
                    .position((50.0, 60.0))
                    .size((20.0, 20.0))
                    .show(ui);
            })
            .node(ui)
    });
    let r = ui.layout[Layer::Main].rect[panel.index()];
    // bbox = max(pos + desired) per axis: 50+20=70, 60+20=80
    assert_eq!(r.size.w, 70.0);
    assert_eq!(r.size.h, 80.0);
}

#[test]
fn canvas_negative_position_does_not_extend_bbox() {
    // Canvas measures `max(pos + desired)` starting at zero, so children
    // placed at negative coords don't grow the panel — they just bleed past
    // the inner top-left.
    let mut ui = new_ui();
    let panel = ui.under_outer(UVec2::new(400, 400), |ui| {
        Panel::canvas()
            .auto_id()
            .size((Sizing::Hug, Sizing::Hug))
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("neg")
                    .position((-5.0, -5.0))
                    .size((20.0, 20.0))
                    .show(ui);
            })
            .node(ui)
    });
    let r = ui.layout[Layer::Main].rect[panel.index()];
    // pos + desired = (15, 15) per axis.
    assert_eq!(r.size.w, 15.0);
    assert_eq!(r.size.h, 15.0);

    let panel_rect = ui.layout[Layer::Main].rect[panel.index()];
    let kids: Vec<_> = ui
        .forest
        .tree(Layer::Main)
        .children(panel)
        .map(|c| c.id)
        .collect();
    let child = ui.layout[Layer::Main].rect[kids[0].index()];
    assert_eq!(child.min.x - panel_rect.min.x, -5.0);
    assert_eq!(child.min.y - panel_rect.min.y, -5.0);
}

/// A constrained Canvas (Fixed) passes its inner to children, so a Fill
/// child takes the canvas's full inner. A Hug Canvas passes INF on Hug
/// axes → Fill falls back to intrinsic (zero for an empty Frame). This
/// preserves the recursive-sizing protection for Hug parents.
///
/// The old "Fill = 0 in Canvas" rule (Fill always intrinsic) was a
/// Canvas-specific quirk that broke constraint propagation for Hug Grid
/// children. Authors who genuinely want "no Fill behavior" can use
/// `Sizing::Hug`.
#[test]
fn canvas_fill_child_uses_inner_when_constrained_else_intrinsic() {
    let cases: &[(&str, Option<f32>, f32)] = &[
        ("fixed_canvas_passes_inner", Some(100.0), 100.0),
        ("hug_canvas_falls_back_to_intrinsic", None, 0.0),
    ];
    for (label, fixed_size, expected) in cases {
        let mut ui = new_ui();
        let panel = ui.under_outer(UVec2::new(400, 400), |ui| {
            let mut canvas = Panel::canvas().auto_id();
            if let Some(s) = *fixed_size {
                canvas = canvas.size((Sizing::Fixed(s), Sizing::Fixed(s)));
            }
            canvas
                .show(ui, |ui| {
                    Frame::new()
                        .id_salt("filler")
                        .position((10.0, 10.0))
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui);
                })
                .node(ui)
        });
        let kids: Vec<_> = ui
            .forest
            .tree(Layer::Main)
            .children(panel)
            .map(|c| c.id)
            .collect();
        let f = ui.layout[Layer::Main].rect[kids[0].index()];
        assert_eq!(f.size.w, *expected, "case: {label} w");
        assert_eq!(f.size.h, *expected, "case: {label} h");
    }
}

#[test]
fn canvas_collapsed_child_does_not_grow_bbox() {
    let mut ui = new_ui();
    let panel = ui.under_outer(UVec2::new(400, 400), |ui| {
        Panel::canvas()
            .auto_id()
            .size((Sizing::Hug, Sizing::Hug))
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("a")
                    .position((0.0, 0.0))
                    .size((10.0, 10.0))
                    .show(ui);
                Frame::new()
                    .id_salt("collapsed")
                    .position((100.0, 100.0))
                    .size((50.0, 50.0))
                    .collapsed()
                    .show(ui);
            })
            .node(ui)
    });
    let r = ui.layout[Layer::Main].rect[panel.index()];
    assert_eq!(r.size.w, 10.0);
    assert_eq!(r.size.h, 10.0);
}

/// Pin: Canvas places children at their explicit `.position(...)` and
/// **ignores `.align(...)`** — children's alignment values do not
/// participate in placement (Canvas is the "explicit position wins"
/// driver). Stack/ZStack/Grid all consume align via `place_axis`;
/// Canvas does not. Adding align-cascade to Canvas would seem like a
/// reasonable change but would break the contract that Canvas users
/// rely on for free-form placement.
#[test]
fn canvas_ignores_child_align() {
    let mut ui = new_ui();
    let mut child = None;
    let _panel = ui.under_outer(UVec2::new(400, 400), |ui| {
        Panel::canvas()
            .auto_id()
            .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
            .show(ui, |ui| {
                child = Some(
                    Frame::new()
                        .id_salt("aligned")
                        .position((30.0, 40.0))
                        .size((50.0, 50.0))
                        // Right/Bottom would matter on Stack/ZStack/Grid;
                        // Canvas must ignore it.
                        .align(Align::new(HAlign::Right, VAlign::Bottom))
                        .show(ui)
                        .node(ui),
                );
            })
            .node(ui)
    });
    let r = ui.layout[Layer::Main].rect[child.unwrap().index()];
    assert_eq!((r.min.x, r.min.y), (30.0, 40.0));
    assert_eq!((r.size.w, r.size.h), (50.0, 50.0));
}
