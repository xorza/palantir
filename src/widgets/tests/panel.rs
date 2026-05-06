use crate::layout::types::sizing::Sizing;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::support::testing::{click_at, ui_at};
use crate::tree::element::Configure;
use crate::widgets::theme::{Background, Surface};
use crate::widgets::{button::Button, frame::Frame, panel::Panel};
use glam::UVec2;

#[test]
fn clip_flag_is_recorded_on_panel_node() {
    // Default is `overflow: visible` — panels do not clip unless asked.
    // Explicit `.background(Surface::scissor())` opts in. Pin both directions so a future
    // default change is loud.
    let mut ui = ui_at(UVec2::new(200, 200));
    let mut default_panel = None;
    let mut opt_in = None;
    Panel::hstack().show(&mut ui, |ui| {
        default_panel = Some(
            Panel::zstack()
                .with_id("default")
                .size(50.0)
                .show(ui, |_| {})
                .node,
        );
        opt_in = Some(
            Panel::zstack()
                .with_id("opt-in")
                .size(50.0)
                .background(Surface::scissor())
                .show(ui, |_| {})
                .node,
        );
    });
    ui.end_frame();

    assert!(
        !ui.tree.paint[default_panel.unwrap().index()]
            .attrs
            .clip_mode()
            .is_clip()
    );
    assert!(
        ui.tree.paint[opt_in.unwrap().index()]
            .attrs
            .clip_mode()
            .is_clip()
    );
}

#[test]
fn panel_hugs_largest_child_and_layers_them() {
    let mut ui = ui_at(UVec2::new(400, 200));
    let mut panel_node = None;
    let mut a_node = None;
    let mut b_node = None;
    Panel::hstack().show(&mut ui, |ui| {
        panel_node = Some(
            Panel::zstack()
                .with_id("card")
                .padding(10.0)
                .background(Background {
                    fill: Color::rgb(0.1, 0.1, 0.15),
                    radius: Corners::all(8.0),
                    ..Default::default()
                })
                .show(ui, |ui| {
                    a_node = Some(
                        Button::new()
                            .with_id("a")
                            .size((Sizing::Fixed(80.0), Sizing::Fixed(30.0)))
                            .show(ui)
                            .node,
                    );
                    b_node = Some(
                        Button::new()
                            .with_id("b")
                            .size((Sizing::Fixed(60.0), Sizing::Fixed(50.0)))
                            .show(ui)
                            .node,
                    );
                })
                .node,
        );
    });
    ui.end_frame();

    // Panel hugs to (max(80, 60) + 2*10, max(30, 50) + 2*10) = (100, 70).
    let panel = ui.pipeline.layout.result.rect[panel_node.unwrap().index()];
    assert_eq!(panel.size.w, 100.0);
    assert_eq!(panel.size.h, 70.0);

    // Both children laid out at panel's inner top-left (10, 10), at their own size.
    let a = ui.pipeline.layout.result.rect[a_node.unwrap().index()];
    let b = ui.pipeline.layout.result.rect[b_node.unwrap().index()];
    assert_eq!((a.min.x, a.min.y), (10.0, 10.0));
    assert_eq!((b.min.x, b.min.y), (10.0, 10.0));
    assert_eq!((a.size.w, a.size.h), (80.0, 30.0));
    assert_eq!((b.size.w, b.size.h), (60.0, 50.0));

    // Panel chrome lives on `extras.chrome`, not in the shapes list.
    let shapes = ui.tree.shapes.slice_of(panel_node.unwrap().index());
    assert!(shapes.is_empty());
    let extras = ui.tree.read_extras(panel_node.unwrap());
    assert!(extras.chrome.is_some(), "panel chrome stamped onto extras");
}

#[test]
fn panel_with_fill_child_grows_to_panel_inner() {
    // Panel with Fixed size + Fill child: child fills panel's inner rect.
    // (Root is an HStack so the panel's Fixed size is honored — root would
    // otherwise expand to surface.)
    let mut ui = ui_at(UVec2::new(400, 400));
    let mut child_node = None;
    Panel::hstack().show(&mut ui, |ui| {
        Panel::zstack()
            .with_id("p")
            .size((Sizing::Fixed(200.0), Sizing::Fixed(100.0)))
            .padding(10.0)
            .show(ui, |ui| {
                child_node = Some(
                    Frame::new()
                        .with_id("filler")
                        .size((Sizing::FILL, Sizing::FILL))
                        .background(Background {
                            fill: Color::rgb(0.5, 0.5, 0.5),
                            ..Default::default()
                        })
                        .show(ui)
                        .node,
                );
            });
    });
    ui.end_frame();

    let child = ui.pipeline.layout.result.rect[child_node.unwrap().index()];
    // Panel = 200×100; inner (after padding 10) = 180×80, child fills it at (10, 10).
    assert_eq!(child.min.x, 10.0);
    assert_eq!(child.min.y, 10.0);
    assert_eq!(child.size.w, 180.0);
    assert_eq!(child.size.h, 80.0);
}

#[test]
fn disabled_panel_suppresses_clicks_on_descendants() {
    use crate::layout::types::display::Display;
    use glam::Vec2;

    let mut ui = ui_at(UVec2::new(400, 200));
    Panel::hstack().show(&mut ui, |ui| {
        Panel::zstack()
            .with_id("locked")
            .size((Sizing::Fixed(200.0), Sizing::Fixed(80.0)))
            .padding(20.0)
            .background(Background {
                fill: Color::rgb(0.2, 0.2, 0.2),
                ..Default::default()
            })
            .disabled(true)
            .show(ui, |ui| {
                Button::new()
                    .with_id("inside")
                    .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                    .show(ui);
            });
    });
    ui.end_frame();

    click_at(&mut ui, Vec2::new(40.0, 40.0));

    ui.begin_frame(Display::default());
    let mut clicked = false;
    Panel::hstack().show(&mut ui, |ui| {
        Panel::zstack()
            .with_id("locked")
            .size((Sizing::Fixed(200.0), Sizing::Fixed(80.0)))
            .padding(20.0)
            .background(Background {
                fill: Color::rgb(0.2, 0.2, 0.2),
                ..Default::default()
            })
            .disabled(true)
            .show(ui, |ui| {
                clicked = Button::new()
                    .with_id("inside")
                    .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                    .show(ui)
                    .clicked();
            });
    });
    assert!(!clicked, "button inside disabled panel should not click");
}
