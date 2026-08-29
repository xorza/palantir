//! The snapshot a frame leaves for the next one to diff against.

use crate::Ui;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::rect::Rect;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::node::configure::Configure;
use crate::ui::harness::UiHarness;
use crate::ui::tests::support::{SURFACE, blue_frame};
use crate::widgets::{button::Button, frame::Frame, panel::Panel};

#[test]
fn prev_frame_empty_before_first_frame() {
    let h = UiHarness::new(SURFACE);
    assert!(h.engines.damage.prev.is_empty());
}

/// Pin the row invariant: after the first frame, widgets with paint
/// rows land in `prev` — painting widgets with their arranged rect and
/// authoring hash, and chromeless parents via their child-marker rows
/// (paint-order tracking), whose all-zero screens union to no paint
/// extent. A rowless node (childless Panel without chrome) stays out.
#[test]
fn prev_frame_captures_nodes_with_rows() {
    let mut h = UiHarness::new(SURFACE);
    let mut frame_node = None;
    h.frame(|ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                frame_node = Some(blue_frame(ui, "a"));
                Panel::hstack()
                    .id(WidgetId::from_hash("empty"))
                    .show(ui, |_| {});
            });
    });
    let frame_node = frame_node.unwrap();
    let prev = &h.engines.damage.prev;
    let snap = &prev[&WidgetId::from_hash("a")];

    assert!(prev.contains_key(&WidgetId::from_hash("root")));
    assert!(!prev.contains_key(&WidgetId::from_hash("empty")));
    // Tracked, and painting nothing: the child-marker rows are all
    // paint-empty and fold through `Rect::union`'s identity, so "no paint
    // extent" is `Rect::ZERO` here as it is everywhere else in the
    // pipeline. `contains_key` above is what answers "is it tracked".
    assert_eq!(
        h.engines
            .damage
            .prev_paint_rect(WidgetId::from_hash("root")),
        Some(Rect::ZERO),
    );
    assert_eq!(
        h.engines
            .damage
            .prev_paint_rect(WidgetId::from_hash("a"))
            .unwrap(),
        h.ui.layout[Layer::Main].rect[frame_node.idx()],
    );
    assert_eq!(
        snap.hash,
        h.ui.forest.trees[Layer::Main].rollups.node[frame_node.idx()],
    );
}

#[test]
fn prev_frame_drops_disappeared_widgets() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Button::new()
                    .id(WidgetId::from_hash("gone"))
                    .label("X")
                    .show(ui);
            });
    });
    assert!(
        h.engines
            .damage
            .prev
            .contains_key(&WidgetId::from_hash("gone"))
    );

    h.frame(|ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |_| {});
    });
    assert!(
        !h.engines
            .damage
            .prev
            .contains_key(&WidgetId::from_hash("gone"))
    );
}

#[test]
fn prev_frame_updates_on_authoring_change() {
    let mut h = UiHarness::new(SURFACE);
    let paint = |fill: Color| {
        move |ui: &mut Ui| {
            Frame::new()
                .id(WidgetId::from_hash("a"))
                .size(50.0)
                .background(Background {
                    fill: fill.into(),
                    ..Default::default()
                })
                .show(ui);
        }
    };
    h.frame(paint(Color::rgb(0.2, 0.4, 0.8)));
    let h1 = h.engines.damage.prev[&WidgetId::from_hash("a")].hash;

    h.frame(paint(Color::rgb(0.9, 0.4, 0.8)));
    let h2 = h.engines.damage.prev[&WidgetId::from_hash("a")].hash;
    assert_ne!(h1, h2);
}
