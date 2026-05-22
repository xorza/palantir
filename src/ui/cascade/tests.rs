use crate::Ui;
use crate::forest::Layer;
use crate::forest::element::Configure;
use crate::layout::types::sizing::Sizing;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::primitives::stroke::Stroke;
use crate::primitives::transform::TranslateScale;
use crate::primitives::widget_id::WidgetId;
use crate::shape::Shape;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

/// A direct shape recorded on a panel with `.transform(...)` must
/// land in `Cascades::paint_arenas` at the *composed* transform
/// (parent ∘ self), not just `parent_transform`. Pins the cascade
/// half of the `Panel::transform`-applies-to-body contract — the
/// encoder half is already pinned by
/// `transformed_panel_applies_transform_to_direct_shapes`.
#[test]
fn shape_rect_composes_self_transform() {
    let scale = 3.0;
    let translate = Vec2::new(10.0, 20.0);
    let xform = TranslateScale::new(translate, scale);

    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(400, 400), |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::canvas()
                .id(WidgetId::from_hash("xpanel"))
                .size(Sizing::Fixed(300.0))
                .transform(xform)
                .show(ui, |ui| {
                    ui.add_shape(Shape::RoundedRect {
                        local_rect: Some(Rect::new(0.0, 0.0, 30.0, 30.0)),
                        corners: Corners::ZERO,
                        fill: Color::rgb(0.5, 0.5, 0.5).into(),
                        stroke: Stroke::ZERO,
                    });
                });
        });
    });

    let layer = Layer::Main;
    let cascades = &ui.layout.cascades;
    let xpanel = cascades.by_id[&WidgetId::from_hash("xpanel")].node;
    let span = cascades.layers[layer].paint_arena.node_spans[xpanel.idx()];
    let shape_rect = cascades.layers[layer].paint_arena.rows[span.start as usize].screen;
    // The Panel sits at the hstack origin (0, 0). Owner-local
    // shape rect is (0, 0, 30, 30); after `parent ∘ self`:
    //   min = (0, 0) * 3 + (10, 20) = (10, 20)
    //   size = (30, 30) * 3 = (90, 90)
    let eps = 1e-3;
    assert!(
        (shape_rect.min.x - 10.0).abs() < eps
            && (shape_rect.min.y - 20.0).abs() < eps
            && (shape_rect.size.w - 90.0).abs() < eps
            && (shape_rect.size.h - 90.0).abs() < eps,
        "expected shape_rect = (10, 20, 90, 90); got {shape_rect:?}",
    );
}

/// `.transform(zoom=S)` on an off-origin panel must anchor the
/// scale at the panel's own `layout_rect.min`, not at the
/// cascade's (0, 0). A child at panel-local (0, 0) should land
/// at the panel's origin regardless of `S` — without anchoring it
/// would slide off by `panel.min * (S - 1)`. Pins the cascade-
/// level half of the "scale my body about my own origin"
/// `Panel::transform` contract.
#[test]
fn self_transform_anchors_scale_at_panel_origin() {
    let zoom = 2.0;
    let xform = TranslateScale::from_scale(zoom);

    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(400, 400), |ui| {
        // Push the transformed panel off the surface origin with a
        // leading sibling — Spacer-style placeholder so the panel
        // sits at (sibling_width, 0) instead of (0, 0).
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("spacer"))
                .size(Sizing::Fixed(50.0))
                .show(ui, |_| {});
            Panel::canvas()
                .id(WidgetId::from_hash("xpanel"))
                .size(Sizing::Fixed(200.0))
                .transform(xform)
                .show(ui, |ui| {
                    ui.add_shape(Shape::RoundedRect {
                        // Panel-local (0, 0) — the natural top-left
                        // of the panel's body.
                        local_rect: Some(Rect::new(0.0, 0.0, 10.0, 10.0)),
                        corners: Corners::ZERO,
                        fill: Color::rgb(0.5, 0.5, 0.5).into(),
                        stroke: Stroke::ZERO,
                    });
                });
        });
    });

    let layer = Layer::Main;
    let cascades = &ui.layout.cascades;
    let xpanel = cascades.by_id[&WidgetId::from_hash("xpanel")].node;
    let span = cascades.layers[layer].paint_arena.node_spans[xpanel.idx()];
    let shape_rect = cascades.layers[layer].paint_arena.rows[span.start as usize].screen;
    // Panel sits at (50, 0). Shape's panel-local (0, 0) should
    // map to screen (50, 0) under the anchor — the panel's own
    // top-left is the fixed point of its scale. Size is
    // `panel-local size * zoom = 10 * 2 = 20`.
    //
    // Without anchoring, the raw `parent.compose(self).apply(panel.min)`
    // would give `(50, 0) * 2 = (100, 0)` — content slides 50px
    // right of where it belongs.
    let eps = 1e-3;
    assert!(
        (shape_rect.min.x - 50.0).abs() < eps && (shape_rect.min.y - 0.0).abs() < eps,
        "expected shape min = (50, 0); got {:?} — scale should anchor at panel.min, \
         not at cascade origin",
        shape_rect.min,
    );
    assert!(
        (shape_rect.size.w - 20.0).abs() < eps && (shape_rect.size.h - 20.0).abs() < eps,
        "expected size = (20, 20) (panel-local * zoom); got {:?}",
        shape_rect.size,
    );
}

/// A panel with chrome emits a Paint row at the start of its
/// node's `node_spans` span; a chromeless panel emits an empty
/// span.
#[test]
fn node_spans_populated_for_chrome_panels_only() {
    use crate::primitives::background::Background;

    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(200, 200), |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("chrome"))
                .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                .background(Background {
                    fill: Color::rgb(0.5, 0.5, 0.5).into(),
                    ..Default::default()
                })
                .show(ui, |_| {});
            Panel::hstack()
                .id(WidgetId::from_hash("bare"))
                .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                .show(ui, |_| {});
        });
    });

    let layer = Layer::Main;
    let cascades = &ui.layout.cascades;
    let chrome_idx = cascades.by_id[&WidgetId::from_hash("chrome")].node.idx();
    let bare_idx = cascades.by_id[&WidgetId::from_hash("bare")].node.idx();
    let chrome_span = cascades.layers[layer].paint_arena.node_spans[chrome_idx];
    let bare_span = cascades.layers[layer].paint_arena.node_spans[bare_idx];

    assert!(
        chrome_span.len > 0
            && cascades.layers[layer].paint_arena.rows[chrome_span.start as usize]
                .screen
                .area()
                > 0.0,
        "chromed panel must have a non-empty paint span with non-zero chrome rect",
    );
    assert_eq!(
        bare_span.len, 0,
        "chromeless panel must have empty paint span; got {bare_span:?}",
    );
}

/// `node_spans` length matches the layer's node count so the
/// damage diff can index by `NodeId.0` without a bounds-cap.
#[test]
fn node_spans_sized_to_node_count() {
    let mut ui = Ui::for_test();
    ui.run_at_acked(UVec2::new(100, 100), |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::hstack().auto_id().show(ui, |_| {});
        });
    });
    let layer = Layer::Main;
    let nodes = ui.forest.tree(Layer::Main).records.len();
    assert_eq!(
        ui.layout.cascades.layers[layer]
            .paint_arena
            .node_spans
            .len(),
        nodes,
        "node_spans column must be sized to the layer's node count",
    );
}
