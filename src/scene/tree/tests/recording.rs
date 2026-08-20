//! What a record pass puts in the arena, and in what order.

use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::rect::Rect;
use crate::primitives::widget_id::WidgetId;
use crate::renderer::frontend::capture::PaintCall;
use crate::scene::layer::Layer;
use crate::scene::node::Configure;
use crate::scene::shapes::paint::QuadShape;
use crate::scene::shapes::record::ShapeRecord;
use crate::scene::tree::node_id::NodeId;
use crate::scene::tree::tests::support::SURFACE;
use crate::shape::Shape;
use crate::shape::rect::{RectKind, RectShape};
use crate::ui::harness::UiHarness;
use crate::widgets::{button::Button, frame::Frame, panel::Panel};

#[test]
fn shapes_attached_to_button_node() {
    let mut h = UiHarness::new(SURFACE);
    let mut button_node = None;
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            button_node = Some(Button::new().auto_id().label("X").show(ui).node());
        });
    });

    // Button chrome lives in `chrome_table`, not in shapes — only the
    // label `Text` shape lands here.
    let shapes: Vec<&ShapeRecord> =
        h.ui.tree(Layer::Main)
            .shapes_of(button_node.unwrap())
            .collect();
    assert_eq!(shapes.len(), 1);
    assert!(matches!(shapes[0], ShapeRecord::Text { .. }));
    assert!(
        h.ui.tree(Layer::Main)
            .chrome(button_node.unwrap())
            .is_some(),
    );
}

/// Pin record-order interleaving: shapes interleaved with child nodes
/// under one parent surface as `shapes.start` values between parent
/// shape indices in the flat buffer; the encoder paints them in that
/// order.
#[test]
fn interleaved_shapes_record_correct_order() {
    fn pos_rect(slot: u16) -> RectShape {
        let s = (slot + 1) as f32 * 10.0;
        Shape::rect(Rect::new(0.0, 0.0, s, s)).fill(Color::rgb(1.0, 0.0, 0.0))
    }
    let mut h = UiHarness::new(SURFACE);
    let p = h.frame_value(|ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
            .show(ui, |ui| {
                ui.add_shape(pos_rect(0));
                Frame::new()
                    .id(WidgetId::from_hash("c0"))
                    .background(Background {
                        fill: Color::rgb(0.0, 1.0, 0.0).into(),
                        ..Default::default()
                    })
                    .size((Sizing::fixed(20.0), Sizing::fixed(20.0)))
                    .show(ui);
                ui.add_shape(pos_rect(1));
                Frame::new()
                    .id(WidgetId::from_hash("c1"))
                    .background(Background {
                        fill: Color::rgb(0.0, 0.0, 1.0).into(),
                        ..Default::default()
                    })
                    .size((Sizing::fixed(20.0), Sizing::fixed(20.0)))
                    .show(ui);
                ui.add_shape(pos_rect(2));
            })
            .response
            .node()
    });
    let pi = p.idx();
    let p_shapes = h.ui.tree(Layer::Main).records.shape_span()[pi];
    assert_eq!(p_shapes.len, 3);
    let children: Vec<_> = h.main_child_ids(p);
    assert_eq!(children.len(), 2);
    let c0_shapes = h.ui.tree(Layer::Main).records.shape_span()[children[0].idx()];
    let c1_shapes = h.ui.tree(Layer::Main).records.shape_span()[children[1].idx()];
    assert_eq!(c0_shapes.start, p_shapes.start + 1);
    assert_eq!(c1_shapes.start, p_shapes.start + 2);
    assert_eq!(
        p_shapes.start + p_shapes.len,
        c1_shapes.start + c1_shapes.len + 1
    );
    let sizes: Vec<f32> =
        h.ui.tree(Layer::Main)
            .shapes_of(p)
            .map(|s| match s {
                ShapeRecord::Quad(QuadShape::Rect {
                    kind: RectKind::Rounded,
                    local_rect: Some(rect),
                    ..
                }) => rect.size.w,
                _ => panic!("unexpected shape variant"),
            })
            .collect();
    assert_eq!(sizes, vec![10.0, 20.0, 30.0]);

    let cmds = h.encode_paint();
    let draw_rect_count = cmds
        .calls
        .iter()
        .filter(|command| matches!(command, PaintCall::Quad(_)))
        .count();
    assert_eq!(
        draw_rect_count, 5,
        "3 parent shapes interleaved with 2 child chromes",
    );
}

/// Regression: `subtree_shape_count` must stay correct when a parent
/// pushes shapes after its only child closes (slot=N). Mirrors the
/// scrollbar pattern: `Scroll` has a single `Body` child, then pushes
/// bar `sub-rect`s at slot N. Without the fix, `nodes[Body].shapes.len`
/// over-counts the bars and the encoder cursor overshoots.
#[test]
fn parent_post_child_shapes_dont_inflate_child_subtree_count() {
    fn pos_rect() -> RectShape {
        Shape::rect(Rect::new(0.0, 0.0, 10.0, 10.0)).fill(Color::rgb(1.0, 0.0, 0.0))
    }
    let mut h = UiHarness::new(SURFACE);
    let mut child_id = None;
    let mut parent_id = None;
    h.frame(|ui| {
        parent_id = Some(
            Panel::vstack()
                .auto_id()
                .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
                .show(ui, |ui| {
                    child_id = Some(
                        Frame::new()
                            .id(WidgetId::from_hash("only-child"))
                            .background(Background {
                                fill: Color::rgb(0.0, 1.0, 0.0).into(),
                                ..Default::default()
                            })
                            .size((Sizing::fixed(20.0), Sizing::fixed(20.0)))
                            .show(ui)
                            .node(),
                    );
                    ui.add_shape(pos_rect());
                    ui.add_shape(pos_rect());
                })
                .response
                .node(),
        );
    });
    let parent = parent_id.unwrap().idx();
    let child = child_id.unwrap().idx();

    assert_eq!(
        h.ui.tree(Layer::Main).records.subtree_end()[parent],
        h.ui.tree(Layer::Main).records.subtree_end()[child],
        "test setup: parent's only child shares the parent's end NodeId"
    );
    assert_eq!(h.ui.tree(Layer::Main).records.shape_span()[parent].len, 2);
    assert_eq!(
        h.ui.tree(Layer::Main).records.shape_span()[child].len,
        0,
        "child's subtree must NOT include parent's slot-N shapes"
    );

    // Encoder walks without panicking (the original symptom).
    let _cmds = h.encode_paint();
}

/// `.gap(...)` is panel-only → populates `panel.table` only;
/// `.min_size(...)` populates `bounds.table` only. Pin so a future
/// re-merge or setter mis-routing trips here.
#[test]
fn extras_columns_split_by_field_kind() {
    use crate::primitives::size::Size;

    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("panel-with-gap"))
            .gap(8.0)
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("leaf-with-min"))
                    .min_size(Size::new(20.0, 20.0))
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("plain-leaf"))
                    .size(10.0)
                    .show(ui);
            });
    });
    assert_eq!(h.ui.tree(Layer::Main).panel_table.len(), 1);
    assert_eq!(h.ui.tree(Layer::Main).bounds_table.len(), 1);
}

#[test]
fn child_iter_traverses_correctly_after_finalize() {
    let mut h = UiHarness::new(SURFACE);
    let root = h.frame_value(|ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("a"))
                    .size(10.0)
                    .show(ui);
                Panel::hstack()
                    .id(WidgetId::from_hash("inner"))
                    .show(ui, |ui| {
                        Frame::new()
                            .id(WidgetId::from_hash("b"))
                            .size(10.0)
                            .show(ui);
                    });
                Frame::new()
                    .id(WidgetId::from_hash("c"))
                    .size(10.0)
                    .show(ui);
            })
            .response
            .node()
    });
    let kids: Vec<u32> =
        h.ui.tree(Layer::Main)
            .children(root)
            .map(|c| c.id.0)
            .collect();
    // Synthetic viewport at NodeId(0); user "root" at NodeId(1).
    assert_eq!(kids, vec![2, 3, 5], "root's direct children: a, inner, c");
    let inner_kids: Vec<u32> =
        h.ui.tree(Layer::Main)
            .children(NodeId(3))
            .map(|c| c.id.0)
            .collect();
    assert_eq!(inner_kids, vec![4], "inner's direct child: b");
}
