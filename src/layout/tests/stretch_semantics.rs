//! Pin: `Sizing::fill` is a WPF Stretch: content size at measure, fills its slot at arrange.
use crate::internals::harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, button::Button, panel::Panel};
use glam::{UVec2, Vec2};

/// A Hug container holding Fill children sizes to its content, not the grandparent's allocation.
#[test]
fn hug_parent_with_fill_children_hugs_to_content() {
    let mut h = UiHarness::new(UVec2::new(800, 600));
    let node_id = WidgetId::from_hash("hug-parent");
    let button_id = WidgetId::from_hash("button");
    h.frame(|ui| {
        Panel::vstack()
            .id(node_id)
            .size((Sizing::HUG, Sizing::HUG))
            .show(ui, |ui| {
                Button::new()
                    .id(button_id)
                    .label("Hi")
                    .size((Sizing::FILL, Sizing::HUG))
                    .show(ui);
            });
    });
    let parent = h.arranged(node_id);
    let button = h.arranged(button_id);
    assert!(
        parent.size.w < 100.0,
        "Hug parent must hug to content, not balloon to surface; got w={}",
        parent.size.w,
    );
    assert_eq!(
        button.size.w, parent.size.w,
        "Fill child arranges to fill parent's inner; got button.w={} parent.w={}",
        button.size.w, parent.size.w,
    );
}

/// **Pin:** a Fill child inside a Fixed-width parent stretches to the full inner width at arrange.
#[test]
fn fill_child_stretches_to_fixed_parent() {
    let mut h = UiHarness::new(UVec2::new(800, 600));
    let child_id = WidgetId::from_hash("child");
    h.frame(|ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::fixed(400.0), Sizing::HUG))
            .show(ui, |ui| {
                Block::new()
                    .id(child_id)
                    .size((Sizing::FILL, Sizing::fixed(20.0)))
                    .show(ui);
            });
    });
    let r = h.arranged(child_id);
    assert_eq!(r.size.w, 400.0);
}

/// Two equal-weight Fill siblings in a Fixed-width HStack each get half the inner width at arrange.
#[test]
fn equal_weight_fill_siblings_split_fixed_parent_equally() {
    let mut h = UiHarness::new(UVec2::new(800, 600));
    let a = WidgetId::from_hash("a");
    let b = WidgetId::from_hash("b");
    h.frame(|ui| {
        Panel::hstack()
            .auto_id()
            .size((Sizing::fixed(400.0), Sizing::HUG))
            .show(ui, |ui| {
                Block::new()
                    .id(a)
                    .size((Sizing::FILL, Sizing::fixed(20.0)))
                    .show(ui);
                Block::new()
                    .id(b)
                    .size((Sizing::FILL, Sizing::fixed(20.0)))
                    .show(ui);
            });
    });
    let ra = h.arranged(a);
    let rb = h.arranged(b);
    assert_eq!(ra.size.w, 200.0);
    assert_eq!(rb.size.w, 200.0);
}

/// A Hug VStack in a Fill canvas hugs its content, even with an internal Fill row, which arranges to the hugged width.
#[test]
fn hug_node_in_canvas_fill_children_arrange_to_hug_width() {
    let surface = UVec2::new(1600, 800);
    let mut h = UiHarness::with_text(surface);
    let node_id = WidgetId::from_hash("node");
    let row_id = WidgetId::from_hash("row");
    h.frame(|ui| {
        Panel::canvas()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Panel::vstack()
                    .id(node_id)
                    .position(Vec2::new(40.0, 40.0))
                    .size((Sizing::HUG, Sizing::HUG))
                    .show(ui, |ui| {
                        Panel::hstack()
                            .id(row_id)
                            .size((Sizing::FILL, Sizing::HUG))
                            .show(ui, |ui| {
                                Block::new()
                                    .auto_id()
                                    .size((Sizing::fixed(50.0), Sizing::fixed(20.0)))
                                    .show(ui);
                            });
                    });
            });
    });
    let node = h.arranged(node_id);
    let row = h.arranged(row_id);
    assert_eq!(node.size.w, 50.0, "Hug node must hug to content");
    assert_eq!(row.size.w, node.size.w);
}

/// A Hug HStack with a Hug button and a Fill spacer sizes to the button only: the spacer has zero leftover.
#[test]
fn hug_hstack_with_fill_spacer_hugs_to_button() {
    let mut h = UiHarness::new(UVec2::new(400, 100));
    let root = WidgetId::from_hash("root");
    let button = WidgetId::from_hash("button");
    let spacer = WidgetId::from_hash("spacer");
    h.frame(|ui| {
        Panel::hstack().id(root).show(ui, |ui| {
            Button::new().id(button).label("Hi").show(ui);
            Block::new()
                .id(spacer)
                .size((Sizing::FILL, Sizing::HUG))
                .show(ui);
        });
    });
    let r_root = h.arranged(root);
    let r_button = h.arranged(button);
    let r_spacer = h.arranged(spacer);
    assert_eq!(r_root.size.w, r_button.size.w);
    assert_eq!(r_spacer.size.w, 0.0);
}
