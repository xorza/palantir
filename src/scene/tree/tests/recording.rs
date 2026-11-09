//! What a record pass puts in the arena, and in what order.

use crate::host::harness::Harness;
use crate::internals::paint_capture::PaintCall;
use crate::primitives::geometry::coverage::Coverage;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::scene::layer::Layer;
use crate::scene::tree::node_id::NodeId;
use crate::scene::tree::tests::support::SURFACE;
use crate::shape::Shape;
use crate::shape::geometry::box_geometry::BoxGeometry;
use crate::shape::paint::quad_shape::QuadShape;
use crate::shape::record::ShapeRecord;
use crate::shape::rect::RectShape;
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, button::Button, panel::Panel};

#[test]
fn shapes_attached_to_button_node() {
    let mut h = Harness::mono(SURFACE);
    let mut button_node = None;
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            button_node = Some(Button::new().auto_id().label("X").show(ui).node(ui));
        });
    });

    let shapes: Vec<&ShapeRecord> = h
        .ui()
        .tree(Layer::Main)
        .shapes_of(button_node.unwrap())
        .collect();
    assert_eq!(shapes.len(), 1);
    assert!(matches!(shapes[0], ShapeRecord::Text { .. }));
    assert!(
        h.ui()
            .tree(Layer::Main)
            .chrome(button_node.unwrap())
            .is_some(),
    );
}

/// Shapes interleaved with child nodes surface as `shapes.start` values between parent shape indices; the encoder paints in that order.
#[test]
fn interleaved_shapes_record_correct_order() {
    fn pos_rect(slot: u16) -> RectShape {
        let s = f32::from(slot + 1) * 10.0;
        Shape::rect().at(Rect::new(0.0, 0.0, s, s))
    }
    let mut h = Harness::mono(SURFACE);
    let p = h.frame_value(|ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
            .show(ui, |ui| {
                ui.add_shape(pos_rect(0), RgbaF32::srgb(1.0, 0.0, 0.0));
                Block::new()
                    .id(WidgetId::from_hash("c0"))
                    .chrome(Chrome::new(RgbaF32::srgb(0.0, 1.0, 0.0)))
                    .size((Sizing::fixed(20.0), Sizing::fixed(20.0)))
                    .show(ui);
                ui.add_shape(pos_rect(1), RgbaF32::srgb(1.0, 0.0, 0.0));
                Block::new()
                    .id(WidgetId::from_hash("c1"))
                    .chrome(Chrome::new(RgbaF32::srgb(0.0, 0.0, 1.0)))
                    .size((Sizing::fixed(20.0), Sizing::fixed(20.0)))
                    .show(ui);
                ui.add_shape(pos_rect(2), RgbaF32::srgb(1.0, 0.0, 0.0));
            })
            .response
            .node(ui)
    });
    let pi = p.idx();
    let p_shapes = h.ui().tree(Layer::Main).records.shape_span()[pi];
    assert_eq!(p_shapes.len, 3);
    let children: Vec<_> = h.main_child_ids(p);
    assert_eq!(children.len(), 2);
    let c0_shapes = h.ui().tree(Layer::Main).records.shape_span()[children[0].idx()];
    let c1_shapes = h.ui().tree(Layer::Main).records.shape_span()[children[1].idx()];
    assert_eq!(c0_shapes.start, p_shapes.start + 1);
    assert_eq!(c1_shapes.start, p_shapes.start + 2);
    assert_eq!(
        p_shapes.start + p_shapes.len,
        c1_shapes.start + c1_shapes.len + 1
    );
    let sizes: Vec<f32> = h
        .ui()
        .tree(Layer::Main)
        .shapes_of(p)
        .map(|s| match s {
            ShapeRecord::Quad(QuadShape::Box {
                geometry:
                    BoxGeometry {
                        coverage: Coverage::Inside,
                        local_rect: Some(rect),
                        ..
                    },
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

/// Regression: `subtree_shape_count` stays correct when a parent pushes shapes after its only child closes (as `Scroll` does for bars); otherwise the encoder cursor overshoots.
#[test]
fn parent_post_child_shapes_dont_inflate_child_subtree_count() {
    fn pos_rect() -> RectShape {
        Shape::rect().at(Rect::new(0.0, 0.0, 10.0, 10.0))
    }
    let mut h = Harness::mono(SURFACE);
    let mut child_id = None;
    let mut parent_id = None;
    h.frame(|ui| {
        parent_id = Some(
            Panel::vstack()
                .auto_id()
                .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
                .show(ui, |ui| {
                    child_id = Some(
                        Block::new()
                            .id(WidgetId::from_hash("only-child"))
                            .chrome(Chrome::new(RgbaF32::srgb(0.0, 1.0, 0.0)))
                            .size((Sizing::fixed(20.0), Sizing::fixed(20.0)))
                            .show(ui)
                            .node(ui),
                    );
                    ui.add_shape(pos_rect(), RgbaF32::srgb(1.0, 0.0, 0.0));
                    ui.add_shape(pos_rect(), RgbaF32::srgb(1.0, 0.0, 0.0));
                })
                .response
                .node(ui),
        );
    });
    let parent = parent_id.unwrap().idx();
    let child = child_id.unwrap().idx();

    assert_eq!(
        h.ui().tree(Layer::Main).records.subtree_end()[parent],
        h.ui().tree(Layer::Main).records.subtree_end()[child],
        "test setup: parent's only child shares the parent's end NodeId"
    );
    assert_eq!(h.ui().tree(Layer::Main).records.shape_span()[parent].len, 2);
    assert_eq!(
        h.ui().tree(Layer::Main).records.shape_span()[child].len,
        0,
        "child's subtree must NOT include parent's slot-N shapes"
    );

    let _cmds = h.encode_paint();
}

/// `.gap(...)` populates `panel.table` only; `.min_size(...)` populates `bounds.table` only.
#[test]
fn extras_columns_split_by_field_kind() {
    use crate::primitives::geometry::size::Size;

    let mut h = Harness::mono(SURFACE);
    h.frame(|ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("panel-with-gap"))
            .gap(8.0)
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("leaf-with-min"))
                    .min_size(Size::new(20.0, 20.0))
                    .show(ui);
                Block::new()
                    .id(WidgetId::from_hash("plain-leaf"))
                    .size(10.0)
                    .show(ui);
            });
    });
    assert_eq!(h.ui().tree(Layer::Main).panel_table.len(), 1);
    assert_eq!(h.ui().tree(Layer::Main).bounds_table.len(), 1);
}

#[test]
fn child_iter_traverses_correctly_after_finalize() {
    let mut h = Harness::mono(SURFACE);
    let root = h.frame_value(|ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("a"))
                    .size(10.0)
                    .show(ui);
                Panel::hstack()
                    .id(WidgetId::from_hash("inner"))
                    .show(ui, |ui| {
                        Block::new()
                            .id(WidgetId::from_hash("b"))
                            .size(10.0)
                            .show(ui);
                    });
                Block::new()
                    .id(WidgetId::from_hash("c"))
                    .size(10.0)
                    .show(ui);
            })
            .response
            .node(ui)
    });
    let kids: Vec<u32> = h
        .ui()
        .tree(Layer::Main)
        .children(root)
        .map(|c| c.id.0)
        .collect();
    assert_eq!(kids, vec![2, 3, 5], "root's direct children: a, inner, c");
    let inner_kids: Vec<u32> = h
        .ui()
        .tree(Layer::Main)
        .children(NodeId(3))
        .map(|c| c.id.0)
        .collect();
    assert_eq!(inner_kids, vec![4], "inner's direct child: b");
}

/// A plane holds more side-table rows than a 16-bit index names: 65 536 nodes with a Tab index
/// each take 65 536 `bounds_table` rows, one past the 65 535 a 16-bit index held, and the last
/// reads back its own row.
#[test]
fn a_plane_holds_more_side_rows_than_a_16_bit_index_names() {
    use crate::internals::layout_rig::LayoutRig;
    use crate::primitives::geometry::size::Size;
    use crate::widget_core::widget::Widget;

    const NODES: i16 = i16::MAX;
    let mut rig = LayoutRig::new();
    rig.run(Size::new(100.0, 100.0), |rec| {
        rec.node("root", Widget::canvas(), |rec| {
            for pass in 0..2 {
                for tab in 1..=NODES {
                    let id = format!("{pass}.{tab}");
                    rec.leaf(&id, Widget::leaf().tab_idx(tab));
                }
            }
            rec.leaf("second last", Widget::leaf().tab_idx(1));
            rec.leaf("last", Widget::leaf().tab_idx(7));
        });
    });
    let tree = rig.tree(Layer::Main);
    let last = rig.node("last");
    let row = tree.records.extras()[last.idx()]
        .bounds
        .expect("a Tab index takes a bounds row");
    assert_eq!(row.idx(), 2 * NODES as usize + 1);
    assert_eq!(tree.bounds_table[row.idx()].tab_idx, 7);
}
