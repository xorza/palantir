//! How a widget is named, and what happens when two ask for one name.

use crate::Ui;
use crate::primitives::rect::Rect;
use crate::primitives::span::Span;
use crate::primitives::widget_id::WidgetId;
use crate::renderer::frontend::Frontend;
use crate::renderer::render_plan::RenderPlan;
use crate::scene::damage::Damage;
use crate::scene::layer::Layer;
use crate::scene::node::configure::Configure;
use crate::scene::tree::node_id::NodeId;
use crate::ui::harness::UiHarness;
use crate::ui::tests::support::SURFACE;
use crate::widgets::{button::Button, frame::Frame, panel::Panel};
use glam::{UVec2, Vec2};
use std::cell::Cell;

/// The magenta outlines the encoder emitted for this frame's explicit-id
/// collisions, in physical pixels.
///
/// Development-build only, which is why every caller is gated:
/// `encoder::collision_overlay` is `cfg(debug_assertions)`, because a
/// shipped app wants neither magenta over its UI nor the branch that
/// tests for it. `Forest.collisions` carries the pairing in every
/// profile, and `Forest::report_explicit_collision`'s `tracing::error!`
/// carries the diagnosis there.
///
/// Selected by stroke width: the overlay is the only 3 px stroke the
/// encoder emits.
#[cfg(debug_assertions)]
fn collision_outlines(ui: &Ui) -> Vec<Rect> {
    // Share Ui's record store so any mesh/polyline bytes pushed at
    // record time are visible at compose / upload — the WindowDriver
    // wiring for real apps.
    let mut frontend = Frontend::for_test();
    frontend.build(
        ui.frame_scene(),
        RenderPlan {
            clear: ui.theme.window_clear,
            damage: Damage::Full,
        },
    );
    frontend
        .buffer
        .quads
        .iter()
        .filter(|q| q.stroke_width > 2.5 && q.stroke_width < 3.5)
        .map(|q| q.rect)
        .collect()
}

/// Two buttons in one frame, both claiming `"dup"`. The second is
/// disambiguated rather than allowed to corrupt every per-id store.
fn record_duplicate_ids(h: &mut UiHarness) -> (WidgetId, NodeId) {
    let button_node = Cell::new(NodeId(0));
    let duplicate_id = WidgetId::from_hash("dup");
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            let a_node = Button::new().id(duplicate_id).show(ui).node();
            Button::new().id(duplicate_id).show(ui);
            button_node.set(a_node);
        });
    });
    (duplicate_id, button_node.get())
}

/// One `"dup"` in `Main` and one in `Popup`. Ids are per-layer, so the
/// pair still collides.
fn record_cross_layer_duplicate_ids(h: &mut UiHarness) {
    h.frame(|ui| {
        Panel::vstack().auto_id().show(ui, |ui| {
            Button::new().id(WidgetId::from_hash("dup")).show(ui);
        });
        ui.layer(Layer::Popup).show(|ui| {
            Button::new().id(WidgetId::from_hash("dup")).show(ui);
        });
    });
}

/// Two `.id(WidgetId::from_hash("dup"))` calls in one frame would silently
/// corrupt every per-id store. Instead of panicking, `SeenIds::record`
/// disambiguates the second one (same path as auto-id collisions) and
/// `Forest` pairs both colliding nodes via `Forest.collisions`.
#[test]
fn duplicate_explicit_widget_id_disambiguates_and_flags() {
    let mut h = UiHarness::new(UVec2::new(100, 100));
    let (duplicate_id, _) = record_duplicate_ids(&mut h);
    // One collision pair should be recorded, survives until the next
    // `pre_record` so the encoder can read it.
    assert_eq!(
        h.ui.forest.collisions.len(),
        1,
        "expected exactly one explicit collision recorded",
    );
    assert_eq!(
        h.ui.cascade.hit_ids().collect::<Vec<_>>(),
        [duplicate_id, duplicate_id.with(1)],
        "hit rows must retain both resolved IDs rather than the duplicated raw ID",
    );
}

/// The overlay a developer sees: one magenta rect per colliding node,
/// at that node's arranged rect (physical px == logical at scale 1).
#[cfg(debug_assertions)]
#[test]
fn duplicate_explicit_widget_ids_are_outlined() {
    let mut h = UiHarness::new(UVec2::new(100, 100));
    let (_, button_node) = record_duplicate_ids(&mut h);
    let button_rect = h.ui.layout[Layer::Main].rect[button_node.idx()];
    let outlines = collision_outlines(&h.ui);
    assert_eq!(outlines.len(), 2, "expected 2 magenta collision outlines");
    let matched = outlines.iter().any(|rect| {
        (rect.min.x - button_rect.min.x).abs() < 1.0
            && (rect.min.y - button_rect.min.y).abs() < 1.0
            && (rect.size.w - button_rect.size.w).abs() < 1.0
            && (rect.size.h - button_rect.size.h).abs() < 1.0
    });
    assert!(
        matched,
        "no outline matched first button's arranged rect {button_rect:?}; outlines: {outlines:?}",
    );
}

/// An explicit id is per-layer, so `Main` and `Popup` each keep their own
/// resolved id — and the pair records which layer each endpoint sat in.
#[test]
fn cross_layer_explicit_widget_id_collision_resolves_per_layer() {
    let mut h = UiHarness::new(SURFACE);
    record_cross_layer_duplicate_ids(&mut h);
    assert_eq!(
        h.ui.forest.collisions.len(),
        1,
        "expected one collision pair across Main + Popup",
    );
    let pair = h.ui.forest.collisions[0];
    assert_eq!(
        pair.first.layer,
        Layer::Main,
        "first occurrence should be in Main, got {:?}",
        pair.first.layer,
    );
    assert_eq!(
        pair.second.layer,
        Layer::Popup,
        "second occurrence should be in Popup, got {:?}",
        pair.second.layer,
    );
}

/// Each endpoint's outline comes from its own layer's `LayerLayout`, so a
/// cross-layer pair outlines two rects in two different coordinate
/// sources rather than one twice.
#[cfg(debug_assertions)]
#[test]
fn cross_layer_duplicate_widget_ids_are_outlined_per_layer() {
    let mut h = UiHarness::new(SURFACE);
    record_cross_layer_duplicate_ids(&mut h);
    let pair = h.ui.forest.collisions[0];
    let main_rect = h.ui.layout[Layer::Main].rect[pair.first.node.idx()];
    let popup_rect = h.ui.layout[Layer::Popup].rect[pair.second.node.idx()];
    let outlines = collision_outlines(&h.ui);
    assert_eq!(outlines.len(), 2, "expected 2 magenta collision outlines");
    assert!(
        outlines
            .iter()
            .any(|rect| (rect.min - main_rect.min).length() < 1.0),
        "no outline at Main rect {main_rect:?}",
    );
    assert!(
        outlines
            .iter()
            .any(|rect| (rect.min - popup_rect.min).length() < 1.0),
        "no outline at Popup rect {popup_rect:?}",
    );
}

#[test]
fn layout_outputs_stay_isolated_per_layer_across_cache_hits() {
    let mut h = UiHarness::with_text(SURFACE);
    let main_id = WidgetId::from_hash("layer-output-main");
    let popup_id = WidgetId::from_hash("layer-output-popup");

    let mut record = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("layer-output-main-root"))
            .show(ui, |ui| {
                Button::new()
                    .id(main_id)
                    .label("main layer")
                    .size((40.0, 20.0))
                    .show(ui);
            });
        ui.layer(Layer::Popup).at(Vec2::new(80.0, 60.0)).show(|ui| {
            Button::new()
                .id(popup_id)
                .label("popup layer")
                .size((70.0, 30.0))
                .show(ui);
        });
    };
    let node_for = |ui: &Ui, layer: Layer, id: WidgetId| {
        let index = ui.forest.trees[layer]
            .records
            .widget_id()
            .iter()
            .position(|widget_id| *widget_id == id)
            .unwrap();
        NodeId(index as u32)
    };

    h.frame(&mut record);
    let main_node = node_for(&h.ui, Layer::Main, main_id);
    let popup_node = node_for(&h.ui, Layer::Popup, popup_id);
    let cold_main = h.ui.layout[Layer::Main].rect[main_node.idx()];
    let cold_popup = h.ui.layout[Layer::Popup].rect[popup_node.idx()];
    assert_eq!(cold_main, Rect::new(0.0, 0.0, 40.0, 20.0));
    assert_eq!(cold_popup, Rect::new(80.0, 60.0, 70.0, 30.0));

    let main_span = h.ui.layout[Layer::Main].text_spans[main_node.idx()];
    let popup_span = h.ui.layout[Layer::Popup].text_spans[popup_node.idx()];
    assert_eq!(main_span, Span::new(0, 1));
    assert_eq!(popup_span, Span::new(0, 1));
    assert_eq!(h.ui.layout[Layer::Main].text_shapes.len(), 1);
    assert_eq!(h.ui.layout[Layer::Popup].text_shapes.len(), 1);
    let cold_main_key = h.ui.layout[Layer::Main].text_shapes[main_span.start as usize].key;
    let cold_popup_key = h.ui.layout[Layer::Popup].text_shapes[popup_span.start as usize].key;
    assert_ne!(cold_main_key, cold_popup_key);

    h.frame(&mut record);
    assert!(
        !h.engines.layout.scratch.counters.cache_hits().is_empty(),
        "warm frame must exercise measure-cache restoration",
    );
    let main_node = node_for(&h.ui, Layer::Main, main_id);
    let popup_node = node_for(&h.ui, Layer::Popup, popup_id);
    assert_eq!(h.ui.layout[Layer::Main].rect[main_node.idx()], cold_main);
    assert_eq!(h.ui.layout[Layer::Popup].rect[popup_node.idx()], cold_popup);
    assert_eq!(
        h.ui.layout[Layer::Main].text_shapes[main_span.start as usize].key,
        cold_main_key,
    );
    assert_eq!(
        h.ui.layout[Layer::Popup].text_shapes[popup_span.start as usize].key,
        cold_popup_key,
    );
}

/// Pin: the encoder-direct overlay path leaves `Layer::Debug` empty
/// (no sink node recorded) — guards against silent regression back to
/// the prior "sink in Debug" approach.
#[test]
fn collisions_do_not_record_into_debug_layer() {
    let mut h = UiHarness::new(UVec2::new(100, 100));
    assert!(
        !h.ui.resources.diagnostics.overlay.get().frame_stats,
        "test relies on frame_stats off — Debug should otherwise stay empty",
    );
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Button::new().id(WidgetId::from_hash("dup")).show(ui);
            Button::new().id(WidgetId::from_hash("dup")).show(ui);
        });
    });
    assert!(
        !h.ui.forest.collisions.is_empty(),
        "collision should have been recorded",
    );
    assert_eq!(
        h.ui.forest.trees[Layer::Debug].records.len(),
        0,
        "encoder-direct overlay path must not record nodes into Layer::Debug",
    );
}

/// Auto-generated ids (call-site hash) silently disambiguate when the same
/// site fires more than once per frame — the "loop / closure helper" case.
#[test]
fn auto_id_collisions_disambiguate() {
    fn chip(ui: &mut Ui) {
        Frame::new().auto_id().show(ui);
    }
    let mut h = UiHarness::new(UVec2::new(100, 100));
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            chip(ui);
            chip(ui);
            chip(ui);
        });
    });
    // Synthetic viewport root + 1 panel + 3 chips = 5 distinct ids, no panic.
    assert_eq!(h.ui.forest.trees[Layer::Main].records.len(), 5);
}

#[test]
fn state_map_persists_and_evicts_with_recorded_ids() {
    let mut h = UiHarness::new(UVec2::new(100, 100));
    let id_a = WidgetId::from_hash("a");
    let id_b = WidgetId::from_hash("b");

    h.frame(|ui| {
        Frame::new().id(WidgetId::from_hash("a")).show(ui);
        Frame::new().id(WidgetId::from_hash("b")).show(ui);
        *ui.state_mut::<u32>(id_a) = 11;
        *ui.state_mut::<u32>(id_b) = 22;
    });
    h.frame(|ui| {
        Frame::new().id(WidgetId::from_hash("a")).show(ui);
        // Reading state during recording so the row is touched while
        // its widget is still seen.
        assert_eq!(*ui.state_mut::<u32>(id_a), 11);
    });
    h.frame(|ui| {
        Frame::new().id(WidgetId::from_hash("b")).show(ui);
        assert_eq!(
            *ui.state_mut::<u32>(id_b),
            0,
            "B was unrecorded last frame; its row should have been swept",
        );
    });
}
