//! A side layer gets its own tree, and keeps it independent of the one it
//! opened from.

use crate::Ui;
use crate::layout::types::placement::{Placement, PlacementOrigin};
use crate::layout::types::sizing::Sizing;
use crate::primitives::color::Color;
use crate::primitives::rect::Rect;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::node::configure::Configure;
use crate::scene::shapes::paint::QuadShape;
use crate::scene::shapes::record::ShapeRecord;
use crate::scene::tree::tests::support::SURFACE;
use crate::shape::Shape;
use crate::shape::rect::{RectKind, RectShape};
use crate::ui::harness::UiHarness;
use crate::widgets::{button::Button, frame::Frame, panel::Panel};
use glam::UVec2;

#[test]
fn ui_layer_records_popup_into_separate_tree() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    let popup_anchor = glam::Vec2::new(50.0, 60.0);
    h.frame(|ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("main-root"))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("main-leaf"))
                    .size(50.0)
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("main-leaf-2"))
                    .size(30.0)
                    .show(ui);
            });
        ui.layer(Layer::Popup).at(popup_anchor).show(|ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("popup-root"))
                .show(ui, |ui| {
                    Frame::new()
                        .id(WidgetId::from_hash("popup-leaf"))
                        .size(20.0)
                        .show(ui);
                });
        });
    });
    let main_tree = h.ui.tree(Layer::Main);
    let popup_tree = h.ui.tree(Layer::Popup);
    assert_eq!(main_tree.roots.len(), 1);
    assert_eq!(popup_tree.roots.len(), 1);
    assert_eq!(main_tree.roots[0].first_node.idx(), 0);
    assert_eq!(popup_tree.roots[0].first_node.idx(), 0);
    assert!(
        matches!(
            popup_tree.roots[0].placement,
            Placement {
                origin: PlacementOrigin::Anchor(anchor),
                max_size: None
            } if anchor == popup_anchor
        ),
        "popup root keeps its fixed layer placement",
    );
    assert_eq!(
        main_tree.records.subtree_end()[0].end() as usize,
        main_tree.records.len(),
    );
    assert_eq!(
        popup_tree.records.subtree_end()[0].end() as usize,
        popup_tree.records.len(),
    );
}

/// `Ui::layer`'s optional size cap selects the overlay's `available`.
/// `None` fills from anchor to surface bottom-right. `Some(s)` is
/// anchor-independent and clamped to the surface; the caller owns
/// placement in that mode. Anchor here is (50, 40) on a 400×300
/// surface; remaining viewport from that anchor is (350, 260).
#[test]
fn ui_layer_size_caps_overlay_available() {
    use crate::primitives::size::Size;
    const SURF: UVec2 = UVec2::new(400, 300);
    let anchor = glam::Vec2::new(50.0, 40.0);
    let cases: &[(Option<Size>, Size)] = &[
        // None → anchor-clamped: surface − anchor.
        (None, Size::new(350.0, 260.0)),
        // Some(s) → anchor-independent: cap unchanged when ≤ surface.
        (Some(Size::new(120.0, 80.0)), Size::new(120.0, 80.0)),
        // Some(huge) → clamped to the full surface size, not to
        // `surface − anchor` (the caller picks the position).
        (Some(Size::new(9999.0, 9999.0)), Size::new(400.0, 300.0)),
        // Some(mixed) → each axis clamps independently to surface.
        (Some(Size::new(100.0, 9999.0)), Size::new(100.0, 300.0)),
    ];
    let mut h = UiHarness::new(SURF);
    for (cap, expected) in cases {
        h.frame(|ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("main"))
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |_| {});
            let mut scope = ui.layer(Layer::Popup).at(anchor);
            if let Some(cap) = cap {
                scope = scope.max_size(*cap);
            }
            scope.show(|ui| {
                Panel::vstack()
                    .id(WidgetId::from_hash("overlay-root"))
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |_| {});
            });
        });
        let popup_tree = h.ui.tree(Layer::Popup);
        let root = popup_tree.roots[0].first_node.idx();
        let rect = h.ui.layout(Layer::Popup).rect[root];
        assert_eq!(rect.min, anchor, "cap={cap:?}");
        assert_eq!(rect.size, *expected, "cap={cap:?}");
    }
}

#[test]
fn empty_popup_body_leaves_popup_tree_empty() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("only-main"))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("leaf"))
                    .size(20.0)
                    .show(ui);
            });
        ui.layer(Layer::Popup).show(|_| {});
    });
    assert_eq!(h.ui.tree(Layer::Main).roots.len(), 1);
    assert!(h.ui.tree(Layer::Popup).roots.is_empty());
    assert!(h.ui.tree(Layer::Popup).records.is_empty());
}

#[test]
fn forest_independence_across_recording_orders() {
    let popup_anchor = glam::Vec2::new(10.0, 10.0);
    let record_main = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("main-root"))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("main-leaf"))
                    .size(50.0)
                    .show(ui);
            });
    };
    let record_popup = |ui: &mut Ui| {
        ui.layer(Layer::Popup).at(popup_anchor).show(|ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("popup-root"))
                .show(ui, |ui| {
                    Frame::new()
                        .id(WidgetId::from_hash("popup-leaf"))
                        .size(20.0)
                        .show(ui);
                });
        });
    };
    let mut ui_p_first = UiHarness::new(UVec2::new(400, 400));
    ui_p_first.frame(|ui| {
        record_popup(ui);
        record_main(ui);
    });
    let mut ui_m_first = UiHarness::new(UVec2::new(400, 400));
    ui_m_first.frame(|ui| {
        record_main(ui);
        record_popup(ui);
    });
    for layer in [Layer::Main, Layer::Popup] {
        assert_eq!(
            ui_p_first.ui.tree(layer).records.len(),
            ui_m_first.ui.tree(layer).records.len(),
            "{layer:?} record count independent of recording order",
        );
    }
}

#[test]
fn mid_recording_popup_with_text_renders_through_encoder() {
    let mut h = UiHarness::with_text(UVec2::new(400, 400));
    let popup_anchor = glam::Vec2::new(50.0, 100.0);
    h.frame(|ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("outer-main"))
            .show(ui, |ui| {
                Button::new()
                    .id(WidgetId::from_hash("trigger"))
                    .label("menu")
                    .show(ui);
                ui.layer(Layer::Popup).at(popup_anchor).show(|ui| {
                    Panel::vstack()
                        .id(WidgetId::from_hash("popup-body"))
                        .show(ui, |ui| {
                            Button::new()
                                .id(WidgetId::from_hash("popup-item"))
                                .label("copy")
                                .show(ui);
                        });
                });
            });
    });
    let _cmds = h.encode_paint();

    let store = h.ui.record_store();
    let interned_text = store.interned_text();
    let main_tree = h.ui.tree(Layer::Main);
    let popup_tree = h.ui.tree(Layer::Popup);

    let outer_span = main_tree.records.shape_span()[0];
    let main_texts: Vec<&str> = main_tree.shapes.records
        [outer_span.start as usize..(outer_span.start + outer_span.len) as usize]
        .iter()
        .filter_map(|s| match s {
            ShapeRecord::Text { text, .. } => Some(interned_text.resolve(text.span)),
            _ => None,
        })
        .collect();
    assert_eq!(main_texts, vec!["menu"]);

    let popup_root_span = popup_tree.records.shape_span()[0];
    let popup_texts: Vec<&str> = popup_tree.shapes.records
        [popup_root_span.start as usize..(popup_root_span.start + popup_root_span.len) as usize]
        .iter()
        .filter_map(|s| match s {
            ShapeRecord::Text { text, .. } => Some(interned_text.resolve(text.span)),
            _ => None,
        })
        .collect();
    assert_eq!(popup_texts, vec!["copy"]);
}

/// Pins per-tree shape buffer ownership
/// proven by markers pushed at every Main + Popup level — each appears
/// exactly once, in its owning tree, in recording order.
#[test]
fn mid_recording_popup_keeps_trees_independent() {
    fn marker(slot: u8) -> RectShape {
        let w = (slot + 1) as f32;
        Shape::rect(Rect::new(0.0, 0.0, w, w)).fill(Color::rgb(1.0, 0.0, 0.0))
    }
    fn marker_w(s: &ShapeRecord) -> u32 {
        match s {
            ShapeRecord::Quad(QuadShape::Rect {
                kind: RectKind::Rounded,
                local_rect: Some(r),
                ..
            }) => r.size.w as u32,
            _ => panic!("unexpected shape variant"),
        }
    }

    let mut h = UiHarness::new(UVec2::new(400, 400));
    let popup_anchor = glam::Vec2::new(50.0, 60.0);
    let parent = h.frame_value(|ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("main-parent"))
            .show(ui, |ui| {
                ui.add_shape(marker(0));
                Frame::new()
                    .id(WidgetId::from_hash("mc1"))
                    .size(20.0)
                    .show(ui);
                ui.add_shape(marker(1));
                Frame::new()
                    .id(WidgetId::from_hash("mc2"))
                    .size(20.0)
                    .show(ui);
                ui.add_shape(marker(2));
                ui.layer(Layer::Popup).at(popup_anchor).show(|ui| {
                    Panel::vstack()
                        .id(WidgetId::from_hash("popup-root"))
                        .show(ui, |ui| {
                            ui.add_shape(marker(10));
                            Frame::new()
                                .id(WidgetId::from_hash("popup-leaf"))
                                .size(10.0)
                                .show(ui);
                            ui.add_shape(marker(11));
                            Frame::new()
                                .id(WidgetId::from_hash("popup-leaf-2"))
                                .size(10.0)
                                .show(ui);
                        });
                });
                ui.add_shape(marker(3));
                Frame::new()
                    .id(WidgetId::from_hash("mc3"))
                    .size(20.0)
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("mc4"))
                    .size(20.0)
                    .show(ui);
                ui.add_shape(marker(4));
            })
            .response
            .node()
    });
    let main_tree = h.ui.tree(Layer::Main);
    let popup_tree = h.ui.tree(Layer::Popup);

    // Synthetic viewport at NodeId(0); user "main-parent" at NodeId(1).
    assert_eq!(main_tree.records.len(), 6);
    assert_eq!(main_tree.roots.len(), 1);
    assert_eq!(main_tree.roots[0].first_node.idx(), 0);
    assert_eq!(main_tree.records.subtree_end()[parent.idx()].end(), 6);

    let kids: Vec<u32> = main_tree.children(parent).map(|c| c.id.0).collect();
    assert_eq!(kids, vec![2, 3, 4, 5]);

    let widths: Vec<u32> = main_tree.shapes.records.iter().map(marker_w).collect();
    assert_eq!(widths, vec![1, 2, 3, 4, 5]);
    let parent_span = main_tree.records.shape_span()[parent.idx()];
    assert_eq!(parent_span.start, 0);
    assert_eq!(parent_span.len, 5);
    for leaf_idx in [2, 3, 4, 5] {
        assert_eq!(main_tree.records.shape_span()[leaf_idx as usize].len, 0);
    }

    assert_eq!(popup_tree.records.len(), 3);
    assert_eq!(popup_tree.roots.len(), 1);
    assert_eq!(popup_tree.roots[0].first_node.idx(), 0);
    assert_eq!(popup_tree.records.subtree_end()[0].end(), 3);

    let popup_widths: Vec<u32> = popup_tree.shapes.records.iter().map(marker_w).collect();
    assert_eq!(popup_widths, vec![11, 12]);
    let popup_root_span = popup_tree.records.shape_span()[0];
    assert_eq!(popup_root_span.start, 0);
    assert_eq!(popup_root_span.len, 2);
    for leaf_idx in [1, 2] {
        assert_eq!(popup_tree.records.shape_span()[leaf_idx as usize].len, 0);
    }
}
