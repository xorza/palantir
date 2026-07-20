//! Pin: `Sizing::fill` is a **WPF Stretch** — measure-time it reports
//! content size; arrange-time it fills its allocated slot.
//!
//! These tests pin the contract we want, independent of the current
//! implementation. Where an existing test in this crate contradicts
//! one of these, this file wins and the older test is updated.
use crate::Ui;
use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::element::Configure;
use crate::widgets::{button::Button, frame::Frame, panel::Panel};
use glam::{UVec2, Vec2};

/// **Pin (the darkroom node case):** a Hug container that holds Fill
/// children sizes to its content, not to the grandparent's allocation.
/// `Sizing::fill` is *not* a measure-time expansion — it cannot inflate
/// a Hug ancestor.
#[test]
fn hug_parent_with_fill_children_hugs_to_content() {
    let mut ui = Ui::for_test();
    let node_id = WidgetId::from_hash("hug-parent");
    let button_id = WidgetId::from_hash("button");
    ui.run_at_without_baseline(UVec2::new(800, 600), |ui| {
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
    let parent = ui.response_for(node_id).rect.expect("parent arranged");
    let button = ui.response_for(button_id).rect.expect("button arranged");
    // Parent hugs to button's content width — somewhere near the
    // "Hi" label width plus button padding, definitely less than 100.
    assert!(
        parent.size.w < 100.0,
        "Hug parent must hug to content, not balloon to surface; got w={}",
        parent.size.w,
    );
    // Button still stretches to the parent's inner width (its allocated
    // slot) — Stretch arrange semantics.
    assert_eq!(
        button.size.w, parent.size.w,
        "Fill child arranges to fill parent's inner; got button.w={} parent.w={}",
        button.size.w, parent.size.w,
    );
}

/// **Pin:** Fill child inside a Fixed-width parent stretches to the
/// full inner width at arrange. Pre-existing behavior; pinned here so
/// the Hug-fix doesn't regress it.
#[test]
fn fill_child_stretches_to_fixed_parent() {
    let mut ui = Ui::for_test();
    let child_id = WidgetId::from_hash("child");
    ui.run_at_without_baseline(UVec2::new(800, 600), |ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::fixed(400.0), Sizing::HUG))
            .show(ui, |ui| {
                Frame::new()
                    .id(child_id)
                    .size((Sizing::FILL, Sizing::fixed(20.0)))
                    .show(ui);
            });
    });
    let r = ui.response_for(child_id).rect.expect("child arranged");
    assert_eq!(r.size.w, 400.0);
}

/// **Pin:** two equal-weight Fill siblings in a Fixed-width HStack
/// each get half of the parent's inner width at arrange.
#[test]
fn equal_weight_fill_siblings_split_fixed_parent_equally() {
    let mut ui = Ui::for_test();
    let a = WidgetId::from_hash("a");
    let b = WidgetId::from_hash("b");
    ui.run_at_without_baseline(UVec2::new(800, 600), |ui| {
        Panel::hstack()
            .auto_id()
            .size((Sizing::fixed(400.0), Sizing::HUG))
            .show(ui, |ui| {
                Frame::new()
                    .id(a)
                    .size((Sizing::FILL, Sizing::fixed(20.0)))
                    .show(ui);
                Frame::new()
                    .id(b)
                    .size((Sizing::FILL, Sizing::fixed(20.0)))
                    .show(ui);
            });
    });
    let ra = ui.response_for(a).rect.expect("a arranged");
    let rb = ui.response_for(b).rect.expect("b arranged");
    assert_eq!(ra.size.w, 200.0);
    assert_eq!(rb.size.w, 200.0);
}

/// **Pin (the darkroom canvas node case):** a Hug-sized VStack
/// positioned inside a Fill canvas hugs to its content, even when its
/// internal layout uses Fill rows and Fill columns. Companion to
/// `fill_propagation::hug_node_in_canvas_with_fill_row_does_not_balloon`
/// — that test asserts the Hug node doesn't balloon; this one extends
/// to checking the children arrange correctly inside the hugged width.
#[test]
fn hug_node_in_canvas_fill_children_arrange_to_hug_width() {
    let surface = UVec2::new(1600, 800);
    let mut ui = Ui::for_test_at_text(surface);
    let node_id = WidgetId::from_hash("node");
    let row_id = WidgetId::from_hash("row");
    ui.run_at(surface, |ui| {
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
                                Frame::new()
                                    .auto_id()
                                    .size((Sizing::fixed(50.0), Sizing::fixed(20.0)))
                                    .show(ui);
                            });
                    });
            });
    });
    let node = ui.response_for(node_id).rect.expect("node arranged");
    let row = ui.response_for(row_id).rect.expect("row arranged");
    // The node hugs to its content (the 50-wide frame), not the
    // surface (1600).
    assert!(
        node.size.w < 200.0,
        "Hug node must hug to content; got w={}",
        node.size.w,
    );
    // The Fill row stretches to the node's inner width.
    assert_eq!(row.size.w, node.size.w);
}

/// **Pin:** a Hug HStack containing a Hug button and a Fill spacer
/// sizes to the button's width only (WPF DesiredSize semantics for
/// Stretch is content). The spacer contributes nothing to measure;
/// it only consumes leftover at arrange — but in a Hug parent the
/// arranged size equals the content size, so the spacer has zero
/// leftover to fill.
#[test]
fn hug_hstack_with_fill_spacer_hugs_to_button() {
    let mut ui = Ui::for_test();
    let root = WidgetId::from_hash("root");
    let button = WidgetId::from_hash("button");
    let spacer = WidgetId::from_hash("spacer");
    ui.run_at_without_baseline(UVec2::new(400, 100), |ui| {
        Panel::hstack().id(root).show(ui, |ui| {
            Button::new().id(button).label("Hi").show(ui);
            Frame::new()
                .id(spacer)
                .size((Sizing::FILL, Sizing::HUG))
                .show(ui);
        });
    });
    let r_root = ui.response_for(root).rect.expect("root");
    let r_button = ui.response_for(button).rect.expect("button");
    let r_spacer = ui.response_for(spacer).rect.expect("spacer");
    // Root hugs to the button — no expansion via the Fill spacer.
    assert_eq!(r_root.size.w, r_button.size.w);
    // The spacer in a Hug parent has zero leftover.
    assert_eq!(r_spacer.size.w, 0.0);
}
