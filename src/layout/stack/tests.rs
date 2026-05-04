use crate::layout::types::{align::Align, sizing::Sizing};
use crate::primitives::rect::Rect;
use crate::support::testing::ui_at;
use crate::tree::element::Configure;
use crate::widgets::{button::Button, frame::Frame, panel::Panel};
use glam::UVec2;

#[test]
fn hstack_arranges_two_buttons_side_by_side() {
    let mut ui = ui_at(UVec2::new(800, 600));

    let root = Panel::hstack()
        .show(&mut ui, |ui| {
            Button::new().label("Hi").show(ui);
            Button::new()
                .label("World")
                .size((100.0, Sizing::Hug))
                .show(ui);
        })
        .node;

    ui.end_frame();

    assert_eq!(
        ui.layout_engine.result.rect[root.index()],
        Rect::new(0.0, 0.0, 800.0, 600.0)
    );

    let kids: Vec<_> = ui.tree.children(root).collect();
    assert_eq!(kids.len(), 2);

    // "Hi" measures 2*8=16 wide, 16 tall via the placeholder text metrics.
    // Button has no default padding, so its desired size matches the label.
    let a = ui.layout_engine.result.rect[kids[0].index()];
    assert_eq!(a.min.x, 0.0);
    assert_eq!(a.min.y, 0.0);
    assert_eq!(a.size.w, 16.0);
    assert_eq!(a.size.h, 16.0);

    let b = ui.layout_engine.result.rect[kids[1].index()];
    assert_eq!(b.min.x, 16.0);
    assert_eq!(b.size.w, 100.0);
    assert_eq!(b.size.h, 16.0);
}

#[test]
fn vstack_with_fill_distributes_remainder() {
    let mut ui = ui_at(UVec2::new(200, 300));

    let root = Panel::vstack()
        .show(&mut ui, |ui| {
            Button::new().size((Sizing::Hug, 50.0)).show(ui);
            Button::new().size((Sizing::Hug, Sizing::FILL)).show(ui);
        })
        .node;

    ui.end_frame();

    let kids: Vec<_> = ui.tree.children(root).collect();
    let fixed = ui.layout_engine.result.rect[kids[0].index()];
    let filler = ui.layout_engine.result.rect[kids[1].index()];

    assert_eq!(fixed.size.h, 50.0);
    assert_eq!(filler.min.y, 50.0);
    assert_eq!(filler.size.h, 250.0);
}

#[test]
fn hstack_fill_weights_split_remainder_proportionally() {
    let mut ui = ui_at(UVec2::new(400, 100));
    let root = Panel::hstack()
        .show(&mut ui, |ui| {
            Frame::new()
                .with_id("a")
                .size((Sizing::Fill(1.0), Sizing::Hug))
                .show(ui);
            Frame::new()
                .with_id("b")
                .size((Sizing::Fill(3.0), Sizing::Hug))
                .show(ui);
        })
        .node;
    ui.end_frame();

    let kids: Vec<_> = ui.tree.children(root).collect();
    let a = ui.layout_engine.result.rect[kids[0].index()];
    let b = ui.layout_engine.result.rect[kids[1].index()];
    // 400 leftover / 4 weight = 100 per weight unit → a=100, b=300.
    assert_eq!(a.size.w, 100.0);
    assert_eq!(b.size.w, 300.0);
    assert_eq!(b.min.x, 100.0);
}

#[test]
fn hstack_equal_fill_siblings_are_equal_width_regardless_of_content() {
    let mut ui = ui_at(UVec2::new(400, 100));
    let root = Panel::hstack()
        .show(&mut ui, |ui| {
            Button::new()
                .with_id("wide")
                .label("wide button")
                .size((Sizing::FILL, Sizing::Hug))
                .show(ui);
            Button::new()
                .with_id("narrow")
                .label("x")
                .size((Sizing::FILL, Sizing::Hug))
                .show(ui);
        })
        .node;
    ui.end_frame();

    let kids: Vec<_> = ui.tree.children(root).collect();
    let a = ui.layout_engine.result.rect[kids[0].index()];
    let b = ui.layout_engine.result.rect[kids[1].index()];
    assert_eq!(a.size.w, 200.0);
    assert_eq!(b.size.w, 200.0);
    assert_eq!(a.min.x, 0.0);
    assert_eq!(b.min.x, 200.0);
}

#[test]
fn hstack_justify_center_centers_content_block() {
    use crate::layout::types::justify::Justify;
    let mut ui = ui_at(UVec2::new(200, 100));
    let root = Panel::hstack()
        .justify(Justify::Center)
        .show(&mut ui, |ui| {
            Frame::new().with_id("a").size(40.0).show(ui);
            Frame::new().with_id("b").size(40.0).show(ui);
        })
        .node;
    ui.end_frame();

    let kids: Vec<_> = ui.tree.children(root).collect();
    // Two 40-wide children, no gap → content width = 80. Leftover = 120,
    // half = 60 padding on the leading edge.
    assert_eq!(ui.layout_engine.result.rect[kids[0].index()].min.x, 60.0);
    assert_eq!(ui.layout_engine.result.rect[kids[1].index()].min.x, 100.0);
}

#[test]
fn hstack_justify_end_packs_to_trailing_edge() {
    use crate::layout::types::justify::Justify;
    let mut ui = ui_at(UVec2::new(200, 100));
    let root = Panel::hstack()
        .justify(Justify::End)
        .show(&mut ui, |ui| {
            Frame::new().with_id("a").size(40.0).show(ui);
            Frame::new().with_id("b").size(40.0).show(ui);
        })
        .node;
    ui.end_frame();

    let kids: Vec<_> = ui.tree.children(root).collect();
    // Last child ends at 200; 40 wide → starts at 160. First at 120.
    assert_eq!(ui.layout_engine.result.rect[kids[0].index()].min.x, 120.0);
    assert_eq!(ui.layout_engine.result.rect[kids[1].index()].min.x, 160.0);
}

#[test]
fn hstack_justify_space_between_distributes_leftover_between() {
    use crate::layout::types::justify::Justify;
    let mut ui = ui_at(UVec2::new(200, 100));
    let root = Panel::hstack()
        .justify(Justify::SpaceBetween)
        .show(&mut ui, |ui| {
            Frame::new().with_id("a").size(40.0).show(ui);
            Frame::new().with_id("b").size(40.0).show(ui);
            Frame::new().with_id("c").size(40.0).show(ui);
        })
        .node;
    ui.end_frame();

    let kids: Vec<_> = ui.tree.children(root).collect();
    // Leftover = 200 - 120 = 80, split into 2 gaps of 40.
    assert_eq!(ui.layout_engine.result.rect[kids[0].index()].min.x, 0.0);
    assert_eq!(ui.layout_engine.result.rect[kids[1].index()].min.x, 80.0);
    assert_eq!(ui.layout_engine.result.rect[kids[2].index()].min.x, 160.0);
}

#[test]
fn hstack_justify_space_around_distributes_with_half_pads() {
    use crate::layout::types::justify::Justify;
    let mut ui = ui_at(UVec2::new(200, 100));
    let root = Panel::hstack()
        .justify(Justify::SpaceAround)
        .show(&mut ui, |ui| {
            Frame::new().with_id("a").size(40.0).show(ui);
            Frame::new().with_id("b").size(40.0).show(ui);
        })
        .node;
    ui.end_frame();

    let kids: Vec<_> = ui.tree.children(root).collect();
    // Leftover = 120, /count(2) = 60 per slot. Half (30) padding before first,
    // full 60 between, half (30) after last.
    assert_eq!(ui.layout_engine.result.rect[kids[0].index()].min.x, 30.0);
    // 30 + 40 + 60 = 130
    assert_eq!(ui.layout_engine.result.rect[kids[1].index()].min.x, 130.0);
}

#[test]
fn hstack_justify_is_noop_when_fill_child_consumes_leftover() {
    use crate::layout::types::justify::Justify;
    let mut ui = ui_at(UVec2::new(200, 100));
    let root = Panel::hstack()
        .justify(Justify::Center)
        .show(&mut ui, |ui| {
            Frame::new().with_id("a").size(40.0).show(ui);
            Frame::new()
                .with_id("filler")
                .size((Sizing::FILL, Sizing::Hug))
                .show(ui);
            Frame::new().with_id("c").size(40.0).show(ui);
        })
        .node;
    ui.end_frame();

    let kids: Vec<_> = ui.tree.children(root).collect();
    // Fill consumes leftover → first child still pinned to start.
    assert_eq!(ui.layout_engine.result.rect[kids[0].index()].min.x, 0.0);
    assert_eq!(ui.layout_engine.result.rect[kids[1].index()].min.x, 40.0);
    assert_eq!(ui.layout_engine.result.rect[kids[1].index()].size.w, 120.0);
    assert_eq!(ui.layout_engine.result.rect[kids[2].index()].min.x, 160.0);
}

#[test]
fn hstack_gap_inserts_space_between_children() {
    let mut ui = ui_at(UVec2::new(400, 100));
    let root = Panel::hstack()
        .gap(10.0)
        .show(&mut ui, |ui| {
            Frame::new().with_id("a").size(40.0).show(ui);
            Frame::new().with_id("b").size(40.0).show(ui);
            Frame::new().with_id("c").size(40.0).show(ui);
        })
        .node;
    ui.end_frame();

    let kids: Vec<_> = ui.tree.children(root).collect();
    assert_eq!(ui.layout_engine.result.rect[kids[0].index()].min.x, 0.0);
    assert_eq!(ui.layout_engine.result.rect[kids[1].index()].min.x, 50.0);
    assert_eq!(ui.layout_engine.result.rect[kids[2].index()].min.x, 100.0);
}

#[test]
fn hstack_align_center_centers_child_on_cross_axis() {
    let mut ui = ui_at(UVec2::new(200, 100));
    let root = Panel::hstack()
        .size((Sizing::FILL, Sizing::Fixed(100.0)))
        .show(&mut ui, |ui| {
            Frame::new()
                .with_id("c")
                .size((Sizing::Fixed(40.0), Sizing::Fixed(20.0)))
                .align(Align::CENTER)
                .show(ui);
        })
        .node;
    ui.end_frame();

    let kids: Vec<_> = ui.tree.children(root).collect();
    let r = ui.layout_engine.result.rect[kids[0].index()];
    // Cross axis is height (100); child is 20 tall → centered at (100-20)/2 = 40.
    assert_eq!(r.min.y, 40.0);
    assert_eq!(r.size.h, 20.0);
}

#[test]
fn negative_left_margin_spills_outside_slot() {
    // CSS-style negative margin: the widget reserves a smaller slot but renders
    // larger, shifted toward the negative side. Pin the math so future layout
    // tweaks don't regress it.
    let mut ui = ui_at(UVec2::new(200, 100));
    let mut button_node = None;
    let root = Panel::hstack()
        .show(&mut ui, |ui| {
            button_node = Some(
                Button::new()
                    .with_id("spill")
                    .size((Sizing::Fixed(50.0), Sizing::Fixed(30.0)))
                    .margin((-10.0, 0.0, 0.0, 0.0))
                    .show(ui)
                    .node,
            );
        })
        .node;
    ui.end_frame();

    let kids: Vec<_> = ui.tree.children(root).collect();
    assert_eq!(kids.len(), 1);

    // Rendered rect (what the renderer paints, what hit-test uses) is shifted
    // 10px left of the slot and full Fixed-50 wide — i.e. spilled.
    let r = ui.layout_engine.result.rect[button_node.unwrap().index()];
    assert_eq!(r.min.x, -10.0, "rendered rect spills 10px left of slot");
    assert_eq!(r.min.y, 0.0);
    assert_eq!(
        r.size.w, 50.0,
        "Fixed value is the rendered width, margin doesn't shrink it"
    );
    assert_eq!(r.size.h, 30.0);
}

/// Pass-2 must not double-count non-Fill children in `total_main`. A Hug
/// HStack with a Hug button and a Fill frame in a 200-wide parent should
/// hug to 200 (button + Fill share). The buggy version starts pass 2 with
/// `total_main = sum_non_fill_main` and then adds non-Fill children's
/// desired again in the loop, inflating the reported content size.
#[test]
fn hug_hstack_pass2_does_not_double_count_non_fill_children() {
    let mut ui = ui_at(UVec2::new(200, 100));

    let root = Panel::hstack()
        .show(&mut ui, |ui| {
            // Button "Hi" measures 16 wide via the placeholder text metrics.
            Button::new().label("Hi").show(ui);
            Frame::new()
                .with_id("filler")
                .size((Sizing::FILL, Sizing::Hug))
                .show(ui);
        })
        .node;

    ui.end_frame();

    // Correct: 16 (button) + 184 (Fill share) = 200.
    // Buggy: 16 + 16 (double-counted) + 184 = 216.
    assert_eq!(ui.layout_engine.scratch.desired[root.index()].w, 200.0);
}

/// Pin: a collapsed child between two active children does not advance
/// the cursor and does not count toward `total_gap`. The two active
/// children sit `gap` apart, with the collapsed child's zero rect
/// anchored at the cursor between them. Removing the
/// `if collapsed { zero_subtree; continue; }` branch in `stack::arrange`
/// would advance the cursor over a phantom child and place subsequent
/// active siblings at the wrong position.
#[test]
fn hstack_collapsed_child_neither_advances_cursor_nor_consumes_gap() {
    let mut ui = ui_at(UVec2::new(200, 100));

    let root = Panel::hstack()
        .gap(5.0)
        .show(&mut ui, |ui| {
            Frame::new().with_id("a").size((20.0, 20.0)).show(ui);
            // collapsed: 50 px wide, but contributes 0 to layout.
            Frame::new()
                .with_id("hidden")
                .size((50.0, 20.0))
                .collapsed()
                .show(ui);
            Frame::new().with_id("b").size((30.0, 20.0)).show(ui);
        })
        .node;
    ui.end_frame();

    let kids: Vec<_> = ui.tree.children(root).collect();
    let a = ui.layout_engine.result.rect[kids[0].index()];
    let hidden = ui.layout_engine.result.rect[kids[1].index()];
    let b = ui.layout_engine.result.rect[kids[2].index()];

    assert_eq!((a.min.x, a.size.w), (0.0, 20.0));
    // collapsed: zero-size rect at cursor (= a.right). Cursor stays here.
    assert_eq!((hidden.min.x, hidden.size.w), (20.0, 0.0));
    assert_eq!(hidden.size.h, 0.0);
    // b: cursor += gap (one gap, not two) → starts at 25. Width 30.
    assert_eq!((b.min.x, b.size.w), (25.0, 30.0));
}

/// Pin: a Fill child's `max_size` clamps the measure-time main share —
/// `desired` is capped at `max_size` even when the leftover share is
/// larger. (Arrange separately distributes leftover by weight without
/// consulting `max_size`, so the arranged rect can exceed `desired`;
/// see `hstack_fill_clamped_to_min_content_arranges_at_leftover_share`
/// in `widgets::tests` for the symmetric MinContent case.) Removing
/// the `target.min(cap)` line in `stack::measure` pass-2 would let
/// `desired.w` blow past the declared cap.
#[test]
fn hstack_fill_max_size_caps_measured_share() {
    use crate::primitives::size::Size;

    let mut ui = ui_at(UVec2::new(400, 100));

    let mut fill_node = None;
    Panel::hstack()
        .size((Sizing::Fixed(200.0), Sizing::Fixed(40.0)))
        .show(&mut ui, |ui| {
            Frame::new().with_id("fixed").size((20.0, 20.0)).show(ui);
            fill_node = Some(
                Frame::new()
                    .with_id("fill")
                    .size((Sizing::FILL, 20.0))
                    .max_size(Size::new(50.0, f32::INFINITY))
                    .show(ui)
                    .node,
            );
        });
    ui.end_frame();

    // Leftover for Fill share = 200 - 20 = 180. Cap = 50. Measure clamps
    // target to 50 → desired.w = 50.
    let desired = ui.layout_engine.scratch.desired[fill_node.unwrap().index()];
    assert_eq!(
        desired.w, 50.0,
        "Fill measure must clamp to max_size when leftover share > cap"
    );
}
