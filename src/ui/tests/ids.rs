//! How a widget is named, and what happens when two ask for one name.

use crate::Ui;
use crate::common::span::Span;
use crate::internals::harness::UiHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::tree::node_id::NodeId;
use crate::ui::tests::support::SURFACE;
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, button::Button, panel::Panel};
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
    use crate::damage::Damage;
    use crate::renderer::frontend::Frontend;
    use crate::renderer::render_plan::RenderPlan;

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

/// Under a panned and zoomed panel the outline follows the node to where
/// it paints, not where it was laid out. The canvas translates by (30, 20)
/// and scales by 0.5; each duplicate is a 40 px block, at canvas-local
/// (10, 10) and (60, 10). On screen: (10·0.5 + 30, 10·0.5 + 20) =
/// (35, 25) and (60·0.5 + 30, 25) = (60, 25), each 20 px square.
#[cfg(debug_assertions)]
#[test]
fn a_collision_under_a_transform_is_outlined_where_it_paints() {
    use crate::primitives::geometry::translate_scale::TranslateScale;
    use crate::primitives::layout::sizing::Sizing;

    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(|ui| {
        Panel::canvas()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .transform(TranslateScale::new(Vec2::new(30.0, 20.0), 0.5))
            .show(ui, |ui| {
                for x in [10.0, 60.0] {
                    Block::new()
                        .id(WidgetId::from_hash("dup"))
                        .position((x, 10.0))
                        .size(40.0)
                        .show(ui);
                }
            });
    });
    let mut outlines = collision_outlines(&h.ui);
    outlines.sort_by(|a, b| a.min.x.total_cmp(&b.min.x));
    assert_eq!(
        outlines,
        [
            Rect::new(35.0, 25.0, 20.0, 20.0),
            Rect::new(60.0, 25.0, 20.0, 20.0)
        ],
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
    assert_eq!(
        outlines,
        [main_rect, popup_rect],
        "one outline per layer, each on its own copy's rect",
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
        ui.layer(Layer::Popup)
            .fixed_at(Vec2::new(80.0, 60.0))
            .show(|ui| {
                Button::new()
                    .id(popup_id)
                    .label("popup layer")
                    .size((70.0, 30.0))
                    .show(ui);
            });
    };
    let node_for = |h: &UiHarness, layer: Layer, id: WidgetId| {
        let at = h.node_of(id).expect("recorded");
        assert_eq!(at.layer, layer);
        at.node
    };

    h.frame(&mut record);
    let main_node = node_for(&h, Layer::Main, main_id);
    let popup_node = node_for(&h, Layer::Popup, popup_id);
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

    h.engines.layout.forget_last_run();
    h.frame(&mut record);
    assert!(
        !h.engines.layout.scratch.counters.cache_hits().is_empty(),
        "warm frame must exercise measure-cache restoration",
    );
    let main_node = node_for(&h, Layer::Main, main_id);
    let popup_node = node_for(&h, Layer::Popup, popup_id);
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
        !h.ui.resources.diagnostics().overlay.get().frame_stats,
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
        Block::new().auto_id().show(ui);
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
        Block::new().id(WidgetId::from_hash("a")).show(ui);
        Block::new().id(WidgetId::from_hash("b")).show(ui);
        ui.with_state::<u32, _>(id_a, |_, s| *s = 11);
        ui.with_state::<u32, _>(id_b, |_, s| *s = 22);
    });
    let a = h.frame_value(|ui| {
        Block::new().id(WidgetId::from_hash("a")).show(ui);
        // Reading state during recording so the row is touched while
        // its widget is still seen.
        ui.with_state::<u32, _>(id_a, |_, n| *n)
    });
    assert_eq!(a, 11);
    let b = h.frame_value(|ui| {
        Block::new().id(WidgetId::from_hash("b")).show(ui);
        ui.with_state::<u32, _>(id_b, |_, n| *n)
    });
    assert_eq!(
        b, 0,
        "B was unrecorded last frame; its row should have been swept",
    );
}

/// Two widgets from one call site that both resolve before either
/// records — the shape of reading `.state(ui)` on each, then showing
/// both. The second resolution sees the first's reservation, so they
/// get distinct ids and both open instead of the second hitting the
/// duplicate-record panic.
#[test]
fn two_widgets_resolved_before_recording_get_distinct_ids() {
    use crate::widget_core::widget::Widget;

    let mut h = UiHarness::new(SURFACE);
    let ids = h.frame_value(|ui| {
        let make = || Widget::leaf().auto_id();
        let (mut a, mut b) = (make(), make());
        let ids = [a.resolve(ui), b.resolve(ui)];
        a.record(ui, None, |_| {});
        b.record(ui, None, |_| {});
        ids
    });
    assert_ne!(ids[0], ids[1]);
    assert_eq!(
        ids[1],
        ids[0].with(1),
        "positional, like any auto-id collision"
    );
    for id in ids {
        assert!(h.layout_rect(id).is_some(), "{id:?} recorded");
    }
}
