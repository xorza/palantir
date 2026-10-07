//! The snapshot a frame leaves for the next one to diff against.

use crate::Ui;
use crate::internals::harness::UiHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::scene::layer::Layer;
use crate::ui::tests::support::{SURFACE, blue_frame};
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, button::Button, panel::Panel};

#[test]
fn prev_frame_empty_before_first_frame() {
    let h = UiHarness::new(SURFACE);
    assert!(h.engines.damage.prev.is_empty());
}

/// After the first frame, painting widgets land in `prev` with their arranged rect and hash, chromeless parents via child-marker rows (union of paint-empty screens); a rowless node stays out.
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
    let paint = |fill: RgbaF32| {
        move |ui: &mut Ui| {
            Block::new()
                .id(WidgetId::from_hash("a"))
                .size(50.0)
                .background(Background::fill(fill))
                .show(ui);
        }
    };
    h.frame(paint(RgbaF32::srgb(0.2, 0.4, 0.8)));
    let h1 = h.engines.damage.prev[&WidgetId::from_hash("a")].hash;

    h.frame(paint(RgbaF32::srgb(0.9, 0.4, 0.8)));
    let h2 = h.engines.damage.prev[&WidgetId::from_hash("a")].hash;
    assert_ne!(h1, h2);
}

/// A host divides event positions by the scale the current cascade was laid out at: a user-scale write moves it only once a frame lays out at the new scale.
#[test]
fn the_laid_out_scale_moves_with_the_frame_not_the_write() {
    use crate::display::user_scale::UserScale;
    let mut h = UiHarness::cold(glam::UVec2::new(200, 100));
    assert_eq!(h.ui().laid_out_scale(), None, "nothing laid out yet");
    let mut h = UiHarness::new(glam::UVec2::new(200, 100)).scale(2.0);
    h.frame(|_| {});
    assert_eq!(h.ui().laid_out_scale(), Some(2.0));

    let bigger = UserScale::ONE.stepped_up();
    h.ui().set_user_scale(bigger);
    assert_eq!(
        h.ui().laid_out_scale(),
        Some(2.0),
        "a write between frames does not move the cascade's scale",
    );
}
