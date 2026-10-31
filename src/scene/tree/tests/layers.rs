//! A side layer gets its own tree, independent of the one it opened from.

#![expect(
    clippy::cast_sign_loss,
    reason = "test fixtures cast non-negative sizes, coordinates, indices and color channels"
)]

use crate::host::harness::Harness;
use crate::input::sense::Sense;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::placement::{Origin, Placement};
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::scene::layer::Layer;
use crate::scene::plane::Plane;
use crate::scene::tree::tests::support::SURFACE;
use crate::shape::Shape;
use crate::shape::paint::quad_shape::QuadShape;
use crate::shape::record::ShapeRecord;
use crate::shape::rect::{RectKind, RectShape};
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, button::Button, panel::Panel};
use glam::{UVec2, Vec2};

#[test]
fn ui_layer_records_popup_into_separate_tree() {
    let mut h = Harness::mono(UVec2::new(400, 400));
    let popup_anchor = Vec2::new(50.0, 60.0);
    h.frame(|ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("main-root"))
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("main-leaf"))
                    .size(50.0)
                    .show(ui);
                Block::new()
                    .id(WidgetId::from_hash("main-leaf-2"))
                    .size(30.0)
                    .show(ui);
            });
        ui.layer(Layer::Popup).fixed_at(popup_anchor).show(|ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("popup-root"))
                .show(ui, |ui| {
                    Block::new()
                        .id(WidgetId::from_hash("popup-leaf"))
                        .size(20.0)
                        .show(ui);
                });
        });
    });
    let main_tree = h.ui_ref().tree(Layer::Main);
    let popup_tree = h.ui_ref().tree(Layer::Popup);
    assert_eq!(main_tree.roots.len(), 1);
    assert_eq!(popup_tree.roots.len(), 1);
    assert_eq!(main_tree.roots[0].first_node.idx(), 0);
    assert_eq!(popup_tree.roots[0].first_node.idx(), 0);
    assert!(
        matches!(
            popup_tree.roots[0].placement,
            Placement {
                origin: Origin::Fixed(anchor),
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

/// `Ui::layer`'s size cap sets the overlay's `available`: `None` fills from the anchor (50, 40) to the 400×300 surface corner, (350, 260); `Some(s)` ignores the anchor and clamps to the surface.
#[test]
fn ui_layer_size_caps_overlay_available() {
    use crate::primitives::geometry::size::Size;
    const SURF: UVec2 = UVec2::new(400, 300);
    let anchor = Vec2::new(50.0, 40.0);
    let cases: &[(Option<Size>, Size)] = &[
        (None, Size::new(350.0, 260.0)),
        (Some(Size::new(120.0, 80.0)), Size::new(120.0, 80.0)),
        (Some(Size::new(9999.0, 9999.0)), Size::new(400.0, 300.0)),
        (Some(Size::new(100.0, 9999.0)), Size::new(100.0, 300.0)),
    ];
    let mut h = Harness::mono(SURF);
    for (cap, expected) in cases {
        h.frame(|ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("main"))
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |_| {});
            let mut scope = ui.layer(Layer::Popup).fixed_at(anchor);
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
        let popup_tree = h.ui().tree(Layer::Popup);
        let root = popup_tree.roots[0].first_node.idx();
        let rect = h.ui().layout(Layer::Popup).rect[root];
        assert_eq!(rect.min, anchor, "cap={cap:?}");
        assert_eq!(rect.size, *expected, "cap={cap:?}");
    }
}

#[test]
fn empty_popup_body_leaves_popup_tree_empty() {
    let mut h = Harness::mono(SURFACE);
    h.frame(|ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("only-main"))
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("leaf"))
                    .size(20.0)
                    .show(ui);
            });
        ui.layer(Layer::Popup).show(|_| {});
    });
    assert_eq!(h.ui().tree(Layer::Main).roots.len(), 1);
    assert!(h.ui().tree(Layer::Popup).roots.is_empty());
    assert!(h.ui().tree(Layer::Popup).records.is_empty());
}

#[test]
fn forest_independence_across_recording_orders() {
    let popup_anchor = Vec2::new(10.0, 10.0);
    let record_main = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("main-root"))
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("main-leaf"))
                    .size(50.0)
                    .show(ui);
            });
    };
    let record_popup = |ui: &mut Ui| {
        ui.layer(Layer::Popup).fixed_at(popup_anchor).show(|ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("popup-root"))
                .show(ui, |ui| {
                    Block::new()
                        .id(WidgetId::from_hash("popup-leaf"))
                        .size(20.0)
                        .show(ui);
                });
        });
    };
    let mut ui_p_first = Harness::mono(UVec2::new(400, 400));
    ui_p_first.frame(|ui| {
        record_popup(ui);
        record_main(ui);
    });
    let mut ui_m_first = Harness::mono(UVec2::new(400, 400));
    ui_m_first.frame(|ui| {
        record_main(ui);
        record_popup(ui);
    });
    for layer in [Layer::Main, Layer::Popup] {
        assert_eq!(
            ui_p_first.ui().tree(layer).records.len(),
            ui_m_first.ui().tree(layer).records.len(),
            "{layer:?} record count independent of recording order",
        );
    }
}

#[test]
fn mid_recording_popup_with_text_renders_through_encoder() {
    let mut h = Harness::new(UVec2::new(400, 400));
    let popup_anchor = Vec2::new(50.0, 100.0);
    h.frame(|ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("outer-main"))
            .show(ui, |ui| {
                Button::new()
                    .id(WidgetId::from_hash("trigger"))
                    .label("menu")
                    .show(ui);
                ui.layer(Layer::Popup).fixed_at(popup_anchor).show(|ui| {
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

    let store = h.ui_ref().record_store();
    let interned_text = store.interned_text();
    let main_tree = h.ui_ref().tree(Layer::Main);
    let popup_tree = h.ui_ref().tree(Layer::Popup);

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

/// A marker pushed at every Main and Popup level appears once, in its owning tree, in order.
#[test]
fn mid_recording_popup_keeps_trees_independent() {
    fn marker(slot: u8) -> RectShape {
        let w = f32::from(slot + 1);
        Shape::rect(Rect::new(0.0, 0.0, w, w)).fill(RgbaF32::srgb(1.0, 0.0, 0.0))
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

    let mut h = Harness::mono(UVec2::new(400, 400));
    let popup_anchor = Vec2::new(50.0, 60.0);
    let parent = h.frame_value(|ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("main-parent"))
            .show(ui, |ui| {
                ui.add_shape(marker(0));
                Block::new()
                    .id(WidgetId::from_hash("mc1"))
                    .size(20.0)
                    .show(ui);
                ui.add_shape(marker(1));
                Block::new()
                    .id(WidgetId::from_hash("mc2"))
                    .size(20.0)
                    .show(ui);
                ui.add_shape(marker(2));
                ui.layer(Layer::Popup).fixed_at(popup_anchor).show(|ui| {
                    Panel::vstack()
                        .id(WidgetId::from_hash("popup-root"))
                        .show(ui, |ui| {
                            ui.add_shape(marker(10));
                            Block::new()
                                .id(WidgetId::from_hash("popup-leaf"))
                                .size(10.0)
                                .show(ui);
                            ui.add_shape(marker(11));
                            Block::new()
                                .id(WidgetId::from_hash("popup-leaf-2"))
                                .size(10.0)
                                .show(ui);
                        });
                });
                ui.add_shape(marker(3));
                Block::new()
                    .id(WidgetId::from_hash("mc3"))
                    .size(20.0)
                    .show(ui);
                Block::new()
                    .id(WidgetId::from_hash("mc4"))
                    .size(20.0)
                    .show(ui);
                ui.add_shape(marker(4));
            })
            .response
            .node()
    });
    let main_tree = h.ui_ref().tree(Layer::Main);
    let popup_tree = h.ui_ref().tree(Layer::Popup);

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

/// An overlay opened inside another records in the higher layer of the two, one plane above its
/// opener when they share a layer, however deep; its root keeps the layer it asked for. Each row
/// nests `kinds` outermost first and lists the plane each lands on. Every overlay holds a block
/// at the surface origin, so they overlap: they paint in nesting order and the deepest takes the
/// press.
#[test]
fn a_nested_overlay_records_one_plane_above_its_opener() {
    fn nest(ui: &mut Ui, kinds: &[Layer], depth: usize) {
        let Some((&kind, rest)) = kinds.split_first() else {
            return;
        };
        ui.layer(kind).show(|ui| {
            Block::new()
                .id(WidgetId::from_hash(("nested", depth)))
                .size(10.0)
                .sense(Sense::CLICK)
                .chrome(Chrome::fill(RgbaF32::WHITE))
                .show(ui);
            nest(ui, rest, depth + 1);
        });
    }
    let plane = |layer, depth| Plane { layer, depth };
    let rows: &[(&str, &[Layer], &[Plane])] = &[
        (
            "a popup in a popup",
            &[Layer::Popup, Layer::Popup],
            &[plane(Layer::Popup, 0), plane(Layer::Popup, 1)],
        ),
        (
            "a dropdown in a dialog rises to the dialog's layer",
            &[Layer::Modal, Layer::Popup],
            &[plane(Layer::Modal, 0), plane(Layer::Modal, 1)],
        ),
        (
            "a dialog in a dialog",
            &[Layer::Modal, Layer::Modal],
            &[plane(Layer::Modal, 0), plane(Layer::Modal, 1)],
        ),
        (
            "a higher layer starts at its base",
            &[Layer::Popup, Layer::Tooltip],
            &[plane(Layer::Popup, 0), plane(Layer::Tooltip, 0)],
        ),
        (
            "three deep in one layer",
            &[Layer::Modal, Layer::Popup, Layer::Popup],
            &[
                plane(Layer::Modal, 0),
                plane(Layer::Modal, 1),
                plane(Layer::Modal, 2),
            ],
        ),
    ];
    for &(label, kinds, planes) in rows {
        let mut h = Harness::mono(SURFACE);
        h.frame(|ui| nest(ui, kinds, 0));
        let cascade = h.ui_ref().cascade();
        for (depth, (&kind, &want)) in kinds.iter().zip(planes).enumerate() {
            let id = WidgetId::from_hash(("nested", depth));
            let at = cascade.endpoint(id).expect("recorded");
            assert_eq!(at.plane, want, "{label}: overlay {depth}");
            let root = cascade
                .roots
                .iter()
                .find(|row| row.plane == want)
                .expect("the plane has a root");
            assert_eq!(
                root.layer, kind,
                "{label}: overlay {depth} keeps the layer it asked for"
            );
        }
        let deepest = WidgetId::from_hash(("nested", kinds.len() - 1));
        assert_eq!(
            cascade.hit_test_press(Vec2::splat(5.0)).click,
            Some(deepest),
            "{label}: the deepest overlay takes the press",
        );
        let mut paints = Vec::new();
        cascade.owned_paints(h.ui_ref().forest(), &mut paints);
        let painted: Vec<WidgetId> = paints.iter().map(|paint| paint.owner).collect();
        let nested: Vec<WidgetId> = (0..kinds.len())
            .map(|depth| WidgetId::from_hash(("nested", depth)))
            .collect();
        assert_eq!(painted, nested, "{label}: paint order is nesting order");
    }
}

/// A nested overlay with an empty body grows a plane that holds no root; the frame that first
/// grows it, with nothing else changed, lays out and cascades the new plane as an empty one.
#[test]
fn a_new_plane_with_no_root_reaches_every_table() {
    let record = |ui: &mut Ui, nested: bool| {
        ui.layer(Layer::Popup).show(|ui| {
            Block::new()
                .id(WidgetId::from_hash("outer"))
                .size(10.0)
                .show(ui);
            if nested {
                ui.layer(Layer::Popup).show(|_| {});
            }
        });
    };
    let mut h = Harness::mono(SURFACE);
    h.frame(|ui| record(ui, false));
    h.frame(|ui| record(ui, false));
    h.frame(|ui| record(ui, true));
    h.frame(|ui| record(ui, true));
    let nested = Plane {
        layer: Layer::Popup,
        depth: 1,
    };
    assert!(h.ui().tree(nested).records.is_empty());
}
