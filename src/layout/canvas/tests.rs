use crate::Ui;
use crate::element::Configure;
use crate::primitives::{align::Align, align::HAlign, align::VAlign, sizing::Sizing};
use crate::test_support::under_outer;
use crate::widgets::{frame::Frame, panel::Panel};
use glam::UVec2;

#[test]
fn canvas_places_child_at_position_within_inner_rect() {
    let mut ui = Ui::new();
    let panel = under_outer(&mut ui, UVec2::new(400, 400), |ui| {
        Panel::canvas()
            .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
            .padding(10.0)
            .show(ui, |ui| {
                Frame::with_id("a")
                    .position((30.0, 40.0))
                    .size((20.0, 20.0))
                    .show(ui);
            })
            .node
    });
    let panel_rect = ui.layout_engine.result.rect(panel);
    let kids: Vec<_> = ui.tree.children(panel).collect();
    let a = ui.layout_engine.result.rect(kids[0]);
    assert_eq!(a.min.x - panel_rect.min.x, 40.0);
    assert_eq!(a.min.y, 50.0);
    assert_eq!(a.size.w, 20.0);
    assert_eq!(a.size.h, 20.0);
}

#[test]
fn canvas_hugs_to_bounding_box_of_placed_children() {
    let mut ui = Ui::new();
    let panel = under_outer(&mut ui, UVec2::new(400, 400), |ui| {
        Panel::canvas()
            .size((Sizing::Hug, Sizing::Hug))
            .show(ui, |ui| {
                Frame::with_id("a")
                    .position((10.0, 5.0))
                    .size((30.0, 15.0))
                    .show(ui);
                Frame::with_id("b")
                    .position((50.0, 60.0))
                    .size((20.0, 20.0))
                    .show(ui);
            })
            .node
    });
    let r = ui.layout_engine.result.rect(panel);
    // bbox = max(pos + desired) per axis: 50+20=70, 60+20=80
    assert_eq!(r.size.w, 70.0);
    assert_eq!(r.size.h, 80.0);
}

#[test]
fn canvas_negative_position_does_not_extend_bbox() {
    // Canvas measures `max(pos + desired)` starting at zero, so children
    // placed at negative coords don't grow the panel — they just bleed past
    // the inner top-left.
    let mut ui = Ui::new();
    let panel = under_outer(&mut ui, UVec2::new(400, 400), |ui| {
        Panel::canvas()
            .size((Sizing::Hug, Sizing::Hug))
            .show(ui, |ui| {
                Frame::with_id("neg")
                    .position((-5.0, -5.0))
                    .size((20.0, 20.0))
                    .show(ui);
            })
            .node
    });
    let r = ui.layout_engine.result.rect(panel);
    // pos + desired = (15, 15) per axis.
    assert_eq!(r.size.w, 15.0);
    assert_eq!(r.size.h, 15.0);

    let panel_rect = ui.layout_engine.result.rect(panel);
    let kids: Vec<_> = ui.tree.children(panel).collect();
    let child = ui.layout_engine.result.rect(kids[0]);
    assert_eq!(child.min.x - panel_rect.min.x, -5.0);
    assert_eq!(child.min.y - panel_rect.min.y, -5.0);
}

#[test]
fn canvas_fill_child_takes_constrained_canvas_inner() {
    // A constrained Canvas (Fixed/Fill) passes its inner size to
    // children, so a Fill child takes the canvas's full inner. The
    // child's `position` still applies — placed at `pos + inner.size`,
    // which may overflow the canvas's own rect.
    //
    // The old "Fill = 0 in Canvas" rule (Fill falls back to intrinsic
    // unconditionally) was a Canvas-specific quirk that broke
    // constraint propagation for Hug Grid children. Authors who
    // genuinely want "no Fill behavior" can use `Sizing::Hug`.
    //
    // Hug Canvas (no constraint to propagate) still passes INF on Hug
    // axes → Fill falls back to intrinsic. Pinned by
    // `canvas_hug_fill_child_falls_back_to_intrinsic` below.
    let mut ui = Ui::new();
    let panel = under_outer(&mut ui, UVec2::new(400, 400), |ui| {
        Panel::canvas()
            .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
            .show(ui, |ui| {
                Frame::with_id("filler")
                    .position((10.0, 10.0))
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui);
            })
            .node
    });
    let kids: Vec<_> = ui.tree.children(panel).collect();
    let f = ui.layout_engine.result.rect(kids[0]);
    assert_eq!(f.size.w, 100.0);
    assert_eq!(f.size.h, 100.0);
}

#[test]
fn canvas_hug_fill_child_falls_back_to_intrinsic() {
    // Companion to `canvas_fill_child_takes_constrained_canvas_inner`:
    // Hug Canvas still passes INF on Hug axes → Fill children fall back
    // to intrinsic (zero for an empty Frame). This preserves the
    // recursive-sizing protection for Hug parents.
    let mut ui = Ui::new();
    let panel = under_outer(&mut ui, UVec2::new(400, 400), |ui| {
        Panel::canvas()
            // Hug × Hug — default.
            .show(ui, |ui| {
                Frame::with_id("filler")
                    .position((10.0, 10.0))
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui);
            })
            .node
    });
    let kids: Vec<_> = ui.tree.children(panel).collect();
    let f = ui.layout_engine.result.rect(kids[0]);
    assert_eq!(f.size.w, 0.0);
    assert_eq!(f.size.h, 0.0);
}

#[test]
fn canvas_collapsed_child_does_not_grow_bbox() {
    let mut ui = Ui::new();
    let panel = under_outer(&mut ui, UVec2::new(400, 400), |ui| {
        Panel::canvas()
            .size((Sizing::Hug, Sizing::Hug))
            .show(ui, |ui| {
                Frame::with_id("a")
                    .position((0.0, 0.0))
                    .size((10.0, 10.0))
                    .show(ui);
                Frame::with_id("collapsed")
                    .position((100.0, 100.0))
                    .size((50.0, 50.0))
                    .collapsed()
                    .show(ui);
            })
            .node
    });
    let r = ui.layout_engine.result.rect(panel);
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
    let mut ui = Ui::new();
    let mut child = None;
    let _panel = under_outer(&mut ui, UVec2::new(400, 400), |ui| {
        Panel::canvas()
            .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
            .show(ui, |ui| {
                child = Some(
                    Frame::with_id("aligned")
                        .position((30.0, 40.0))
                        .size((50.0, 50.0))
                        // Right/Bottom would matter on Stack/ZStack/Grid;
                        // Canvas must ignore it.
                        .align(Align::new(HAlign::Right, VAlign::Bottom))
                        .show(ui)
                        .node,
                );
            })
            .node
    });
    let r = ui.layout_engine.result.rect(child.unwrap());
    assert_eq!((r.min.x, r.min.y), (30.0, 40.0));
    assert_eq!((r.size.w, r.size.h), (50.0, 50.0));
}
