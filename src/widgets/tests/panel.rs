use crate::forest::element::Configure;
use crate::forest::tree::Layer;
use crate::layout::types::sizing::Sizing;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::support::testing::{click_at, shapes_of, ui_at};
use crate::widgets::theme::Background;
use crate::widgets::{button::Button, frame::Frame, panel::Panel};
use glam::UVec2;

/// `Surface::apply_to` (called by `Panel::show`) writes the clip bit
/// AND records chrome in `Tree::chrome_table` together. One fixture sweeps every Surface
/// configuration: no surface; paint-only via `From<Background>`;
/// `Surface::scissor`; `Surface::clipped`; `Surface::rounded` with
/// non-zero radius; `Surface::rounded` with zero-radius downgrade.
/// Refactors that touch any mode are caught by the table.
#[test]
fn surface_apply_to_sets_clip_bit_and_chrome() {
    use crate::ClipMode;

    let mut ui = ui_at(UVec2::new(200, 200));
    let mut cases: Vec<(&str, crate::forest::tree::NodeId, ClipMode, bool)> = Vec::new();
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        // No surface set anywhere → no clip, no chrome.
        let n = Panel::zstack()
            .id_salt("none")
            .size(50.0)
            .show(ui, |_| {})
            .node;
        cases.push(("none", n, ClipMode::None, false));

        // Paint-only Background via From<Background>: chrome, no clip.
        let n = Panel::zstack()
            .id_salt("paint-only")
            .size(50.0)
            .background(Background {
                fill: Color::rgb(0.5, 0.5, 0.5).into(),
                ..Default::default()
            })
            .show(ui, |_| {})
            .node;
        cases.push(("paint-only", n, ClipMode::None, true));

        // Surface::scissor — clip + transparent paint. Chrome is
        // dropped at install (Tree::open_node filters invisible
        // paint), so only the clip flag survives.
        let n = Panel::zstack()
            .id_salt("scissor")
            .size(50.0)
            .clip_rect()
            .show(ui, |_| {})
            .node;
        cases.push(("scissor", n, ClipMode::Rect, false));

        // Background + Configure::clip_rect — clip + paint.
        let n = Panel::zstack()
            .id_salt("clipped")
            .size(50.0)
            .background(Background {
                fill: Color::rgb(0.2, 0.2, 0.2).into(),
                ..Default::default()
            })
            .clip_rect()
            .show(ui, |_| {})
            .node;
        cases.push(("clipped", n, ClipMode::Rect, true));

        // Background + Configure::clip_rounded with non-zero radius — Rounded survives.
        let n = Panel::zstack()
            .id_salt("rounded")
            .size(50.0)
            .background(Background {
                fill: Color::rgb(0.2, 0.2, 0.2).into(),
                radius: Corners::all(4.0),
                ..Default::default()
            })
            .clip_rounded()
            .show(ui, |_| {})
            .node;
        cases.push(("rounded", n, ClipMode::Rounded, true));

        // Background + clip_rounded with zero radius — Ui::node downgrades.
        let n = Panel::zstack()
            .id_salt("rounded-zero")
            .size(50.0)
            .background(Background {
                fill: Color::rgb(0.2, 0.2, 0.2).into(),
                ..Default::default()
            })
            .clip_rounded()
            .show(ui, |_| {})
            .node;
        cases.push(("rounded-zero", n, ClipMode::Rect, true));
    });
    ui.record_phase();
    ui.paint_phase();
    for (name, id, expected_clip, expects_chrome) in &cases {
        let clip = ui.forest.tree(Layer::Main).records.attrs()[id.index()].clip_mode();
        assert_eq!(clip, *expected_clip, "[{name}] clip mode");
        let chrome = ui.forest.tree(Layer::Main).chrome.get(id.index());
        assert_eq!(
            chrome.is_some(),
            *expects_chrome,
            "[{name}] chrome stamping"
        );
    }
}

#[test]
fn panel_hugs_largest_child_and_layers_them() {
    let mut ui = ui_at(UVec2::new(400, 200));
    let mut panel_node = None;
    let mut a_node = None;
    let mut b_node = None;
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        panel_node = Some(
            Panel::zstack()
                .id_salt("card")
                .padding(10.0)
                .background(Background {
                    fill: Color::rgb(0.1, 0.1, 0.15).into(),
                    radius: Corners::all(8.0),
                    ..Default::default()
                })
                .show(ui, |ui| {
                    a_node = Some(
                        Button::new()
                            .id_salt("a")
                            .size((Sizing::Fixed(80.0), Sizing::Fixed(30.0)))
                            .show(ui)
                            .node,
                    );
                    b_node = Some(
                        Button::new()
                            .id_salt("b")
                            .size((Sizing::Fixed(60.0), Sizing::Fixed(50.0)))
                            .show(ui)
                            .node,
                    );
                })
                .node,
        );
    });
    ui.record_phase();
    ui.paint_phase();
    // Panel hugs to (max(80, 60) + 2*10, max(30, 50) + 2*10) = (100, 70).
    let panel = ui.layout[Layer::Main].rect[panel_node.unwrap().index()];
    assert_eq!(panel.size.w, 100.0);
    assert_eq!(panel.size.h, 70.0);

    // Both children laid out at panel's inner top-left (10, 10), at their own size.
    let a = ui.layout[Layer::Main].rect[a_node.unwrap().index()];
    let b = ui.layout[Layer::Main].rect[b_node.unwrap().index()];
    assert_eq!((a.min.x, a.min.y), (10.0, 10.0));
    assert_eq!((b.min.x, b.min.y), (10.0, 10.0));
    assert_eq!((a.size.w, a.size.h), (80.0, 30.0));
    assert_eq!((b.size.w, b.size.h), (60.0, 50.0));

    // Panel chrome lives in `Tree::chrome_table`, not in the shapes list.
    assert!(
        shapes_of(ui.forest.tree(Layer::Main), panel_node.unwrap())
            .next()
            .is_none(),
        "panel chrome doesn't show up in the shape stream"
    );
    assert!(
        ui.forest
            .tree(Layer::Main)
            .chrome
            .get(panel_node.unwrap().index())
            .is_some(),
        "panel chrome recorded in chrome table",
    );
}

#[test]
fn panel_with_fill_child_grows_to_panel_inner() {
    // Panel with Fixed size + Fill child: child fills panel's inner rect.
    // (Root is an HStack so the panel's Fixed size is honored — root would
    // otherwise expand to surface.)
    let mut ui = ui_at(UVec2::new(400, 400));
    let mut child_node = None;
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        Panel::zstack()
            .id_salt("p")
            .size((Sizing::Fixed(200.0), Sizing::Fixed(100.0)))
            .padding(10.0)
            .show(ui, |ui| {
                child_node = Some(
                    Frame::new()
                        .id_salt("filler")
                        .size((Sizing::FILL, Sizing::FILL))
                        .background(Background {
                            fill: Color::rgb(0.5, 0.5, 0.5).into(),
                            ..Default::default()
                        })
                        .show(ui)
                        .node,
                );
            });
    });
    ui.record_phase();
    ui.paint_phase();
    let child = ui.layout[Layer::Main].rect[child_node.unwrap().index()];
    // Panel = 200×100; inner (after padding 10) = 180×80, child fills it at (10, 10).
    assert_eq!(child.min.x, 10.0);
    assert_eq!(child.min.y, 10.0);
    assert_eq!(child.size.w, 180.0);
    assert_eq!(child.size.h, 80.0);
}

/// Regression: a child recorded inside a `.disabled(true)` panel
/// must see `state.disabled = true` *during recording* on its very
/// first frame. Cascade lags by a frame, so without
/// `Forest::ancestor_disabled` first-frame `response_for` returned
/// `disabled=false`, which made the animation cache snap to the
/// alive look on insertion and animate to disabled on frame 2 —
/// visible in the showcase as a flash of "alive" disabled buttons.
#[test]
fn child_inside_disabled_panel_sees_disabled_at_record_time() {
    use crate::forest::widget_id::WidgetId;
    let mut ui = ui_at(UVec2::new(200, 200));
    let child_id = WidgetId::from_hash("child");
    let mut observed = None;
    Panel::vstack()
        .auto_id()
        .disabled(true)
        .show(&mut ui, |ui| {
            // Query before opening the child — the disabled panel is
            // open in `Tree::open_frames`, child isn't yet.
            observed = Some(ui.response_for(child_id));
            Frame::new().id(child_id).size(10.0).show(ui);
        });
    ui.record_phase();
    ui.paint_phase();
    assert!(
        observed.expect("query ran").disabled,
        "child inside disabled panel must see disabled at record time",
    );
}

#[test]
fn disabled_panel_suppresses_clicks_on_descendants() {
    use crate::layout::types::display::Display;
    use glam::Vec2;

    let mut ui = ui_at(UVec2::new(400, 200));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        Panel::zstack()
            .id_salt("locked")
            .size((Sizing::Fixed(200.0), Sizing::Fixed(80.0)))
            .padding(20.0)
            .background(Background {
                fill: Color::rgb(0.2, 0.2, 0.2).into(),
                ..Default::default()
            })
            .disabled(true)
            .show(ui, |ui| {
                Button::new()
                    .id_salt("inside")
                    .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                    .show(ui);
            });
    });
    ui.record_phase();
    ui.paint_phase();
    click_at(&mut ui, Vec2::new(40.0, 40.0));

    ui.pre_record(Display::default());
    let mut clicked = false;
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        Panel::zstack()
            .id_salt("locked")
            .size((Sizing::Fixed(200.0), Sizing::Fixed(80.0)))
            .padding(20.0)
            .background(Background {
                fill: Color::rgb(0.2, 0.2, 0.2).into(),
                ..Default::default()
            })
            .disabled(true)
            .show(ui, |ui| {
                clicked = Button::new()
                    .id_salt("inside")
                    .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                    .show(ui)
                    .clicked();
            });
    });
    assert!(!clicked, "button inside disabled panel should not click");
}
