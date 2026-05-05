use super::derive_content;
use crate::Ui;
use crate::layout::types::sizing::Sizing;
use crate::primitives::size::Size;
use crate::support::testing::under_outer;
use crate::tree::NodeId;
use crate::tree::element::{Configure, LayoutMode};
use crate::widgets::{frame::Frame, panel::Panel, scroll::Scroll};
use glam::UVec2;

// --- Driver outputs (measure) -----------------------------------------------
// These pin the contract between `scroll::measure_*` and the wrapping
// `measure_dispatch` arm: content extent lands in `result.scroll_content`,
// the viewport's *own* desired stays at zero on the panned axes (so
// `resolve_desired` falls through to the user's `Sizing`).

#[test]
fn scroll_v_records_content_height_and_yields_panned_axis_to_self_sizing() {
    let mut ui = Ui::new();
    let scroll_node = under_outer(&mut ui, UVec2::new(400, 600), |ui| {
        Scroll::vertical()
            .with_id("scroll")
            .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
            .gap(4.0)
            .show(ui, |ui| {
                // Three rows of 28h with 4px gap → 28*3 + 4*2 = 92.
                for i in 0..3u32 {
                    Frame::new()
                        .with_id(("row", i))
                        .size((Sizing::Fixed(180.0), Sizing::Fixed(28.0)))
                        .show(ui);
                }
            })
            .node
    });

    let rect = ui.layout_engine.result.rect[scroll_node.index()];
    let content = ui.layout_engine.result.scroll_content[scroll_node.index()];
    assert_eq!(
        rect.size.h, 200.0,
        "viewport honors Fixed h, ignores content"
    );
    assert_eq!(content.h, 92.0, "stack(Y) sum + (n-1)·gap");
    assert_eq!(content.w, 180.0, "stack(Y) cross = max child width");
}

#[test]
fn scroll_h_records_content_width_and_yields_panned_axis_to_self_sizing() {
    let mut ui = Ui::new();
    let scroll_node = under_outer(&mut ui, UVec2::new(800, 200), |ui| {
        Scroll::horizontal()
            .with_id("scroll")
            .size((Sizing::Fixed(200.0), Sizing::Fixed(60.0)))
            .gap(8.0)
            .show(ui, |ui| {
                // Two cols of 60w with 8 gap → 60*2 + 8 = 128.
                for i in 0..2u32 {
                    Frame::new()
                        .with_id(("col", i))
                        .size((Sizing::Fixed(60.0), Sizing::Fixed(40.0)))
                        .show(ui);
                }
            })
            .node
    });

    let rect = ui.layout_engine.result.rect[scroll_node.index()];
    let content = ui.layout_engine.result.scroll_content[scroll_node.index()];
    assert_eq!(rect.size.w, 200.0);
    assert_eq!(content.w, 128.0);
    assert_eq!(content.h, 40.0);
}

#[test]
fn scroll_xy_records_max_per_axis() {
    // ZStack-flavored: children overlap at (0,0) inside the viewport.
    // Content extent = max child per axis (not sum).
    let mut ui = Ui::new();
    let scroll_node = under_outer(&mut ui, UVec2::new(400, 400), |ui| {
        Scroll::both()
            .with_id("scroll")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
            .show(ui, |ui| {
                Frame::new()
                    .with_id("wide")
                    .size((Sizing::Fixed(300.0), Sizing::Fixed(60.0)))
                    .show(ui);
                Frame::new()
                    .with_id("tall")
                    .size((Sizing::Fixed(80.0), Sizing::Fixed(250.0)))
                    .show(ui);
            })
            .node
    });

    let content = ui.layout_engine.result.scroll_content[scroll_node.index()];
    assert_eq!(content.w, 300.0, "max child width");
    assert_eq!(content.h, 250.0, "max child height");
}

#[test]
fn scroll_with_no_children_records_zero_content() {
    let mut ui = Ui::new();
    let scroll_node = under_outer(&mut ui, UVec2::new(400, 400), |ui| {
        Scroll::vertical()
            .with_id("empty")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
            .show(ui, |_| {})
            .node
    });

    let content = ui.layout_engine.result.scroll_content[scroll_node.index()];
    assert_eq!(content, Size::ZERO);
}

// --- derive_content (cache-hit fallback) ------------------------------------
// The cache-hit path skips the driver and recomputes content extent from
// already-restored children's `desired`. Formula must match the driver's
// output above; pin both directions.

fn build_three_children(ui: &mut Ui) -> NodeId {
    // Use a regular VStack so we can read children's desired without
    // routing through Scroll's dispatch (no scroll_content writes here).
    under_outer(ui, UVec2::new(400, 400), |ui| {
        Panel::vstack()
            .with_id("parent")
            .gap(5.0)
            .show(ui, |ui| {
                Frame::new()
                    .with_id("a")
                    .size((Sizing::Fixed(40.0), Sizing::Fixed(20.0)))
                    .show(ui);
                Frame::new()
                    .with_id("b")
                    .size((Sizing::Fixed(60.0), Sizing::Fixed(30.0)))
                    .show(ui);
                Frame::new()
                    .with_id("c")
                    .size((Sizing::Fixed(50.0), Sizing::Fixed(40.0)))
                    .show(ui);
            })
            .node
    })
}

#[test]
fn derive_content_scrollv_sums_main_max_cross_with_gap() {
    let mut ui = Ui::new();
    let parent = build_three_children(&mut ui);
    // 20 + 30 + 40 + 5*2 = 100
    let content = derive_content(
        &ui.tree,
        &ui.layout_engine.scratch.desired,
        parent,
        LayoutMode::ScrollV,
    );
    assert_eq!(content.w, 60.0, "max child width");
    assert_eq!(content.h, 100.0, "sum heights + (n-1)·gap");
}

#[test]
fn derive_content_scrollh_sums_main_max_cross_with_gap() {
    let mut ui = Ui::new();
    let parent = build_three_children(&mut ui);
    // 40 + 60 + 50 + 5*2 = 160
    let content = derive_content(
        &ui.tree,
        &ui.layout_engine.scratch.desired,
        parent,
        LayoutMode::ScrollH,
    );
    assert_eq!(content.w, 160.0);
    assert_eq!(content.h, 40.0);
}

#[test]
fn derive_content_scrollxy_takes_max_per_axis_ignoring_gap() {
    let mut ui = Ui::new();
    let parent = build_three_children(&mut ui);
    let content = derive_content(
        &ui.tree,
        &ui.layout_engine.scratch.desired,
        parent,
        LayoutMode::ScrollXY,
    );
    assert_eq!(content.w, 60.0);
    assert_eq!(content.h, 40.0);
}

#[test]
fn derive_content_no_children_is_zero() {
    let mut ui = Ui::new();
    let parent = under_outer(&mut ui, UVec2::new(400, 400), |ui| {
        Panel::vstack()
            .with_id("empty")
            .gap(7.0)
            .show(ui, |_| {})
            .node
    });
    let content = derive_content(
        &ui.tree,
        &ui.layout_engine.scratch.desired,
        parent,
        LayoutMode::ScrollV,
    );
    assert_eq!(content, Size::ZERO);
}

#[test]
fn derive_content_single_child_skips_gap() {
    // Pin: gap is multiplied by `(n-1)`, so a single child contributes
    // zero gap regardless of the configured value.
    let mut ui = Ui::new();
    let parent = under_outer(&mut ui, UVec2::new(400, 400), |ui| {
        Panel::vstack()
            .with_id("solo")
            .gap(99.0)
            .show(ui, |ui| {
                Frame::new()
                    .with_id("only")
                    .size((Sizing::Fixed(30.0), Sizing::Fixed(40.0)))
                    .show(ui);
            })
            .node
    });
    let content = derive_content(
        &ui.tree,
        &ui.layout_engine.scratch.desired,
        parent,
        LayoutMode::ScrollV,
    );
    assert_eq!(content.h, 40.0, "no gap with one child");
}

#[test]
#[should_panic(expected = "non-scroll mode")]
fn derive_content_panics_on_non_scroll_mode() {
    let mut ui = Ui::new();
    let parent = build_three_children(&mut ui);
    let _ = derive_content(
        &ui.tree,
        &ui.layout_engine.scratch.desired,
        parent,
        LayoutMode::VStack,
    );
}
