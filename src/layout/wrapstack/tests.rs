use crate::Ui;
use crate::element::Configure;
use crate::primitives::{color::Color, justify::Justify, sizing::Sizing};
use crate::test_support::under_outer;
use crate::tree::NodeId;
use crate::widgets::{frame::Frame, panel::Panel, styled::Styled};
use glam::UVec2;

fn cell(ui: &mut Ui, id: &'static str, w: f32, h: f32) -> NodeId {
    Frame::with_id(id)
        .size((Sizing::Fixed(w), Sizing::Fixed(h)))
        .fill(Color::WHITE)
        .show(ui)
        .node
}

/// Pin: three 60×20 cells in a 200-wide WrapHStack with `gap=10` fit on
/// one line (60+10+60+10+60 = 200). All three sit at y=0.
#[test]
fn wrap_hstack_packs_into_single_line_when_fits() {
    let mut ui = Ui::new();
    let mut kids = Vec::new();
    let _wrap = under_outer(&mut ui, UVec2::new(400, 400), |ui| {
        Panel::wrap_hstack_with_id("w")
            .size((Sizing::Fixed(200.0), Sizing::Hug))
            .gap(10.0)
            .line_gap(8.0)
            .show(ui, |ui| {
                kids.push(cell(ui, "a", 60.0, 20.0));
                kids.push(cell(ui, "b", 60.0, 20.0));
                kids.push(cell(ui, "c", 60.0, 20.0));
            })
            .node
    });
    let a = ui.layout_engine.rect(kids[0]);
    let b = ui.layout_engine.rect(kids[1]);
    let c = ui.layout_engine.rect(kids[2]);
    assert_eq!(a.min.y, 0.0);
    assert_eq!(b.min.y, 0.0);
    assert_eq!(c.min.y, 0.0);
    assert_eq!(a.min.x, 0.0);
    assert_eq!(b.min.x, 70.0);
    assert_eq!(c.min.x, 140.0);
}

/// Pin: a 4th 60-wide cell wraps to a new line because 60+10+60+10+60+10+60 = 250 > 200.
/// Lines have height 20; line_gap=8 → 4th cell sits at y=28.
#[test]
fn wrap_hstack_wraps_when_next_child_overflows() {
    let mut ui = Ui::new();
    let mut kids = Vec::new();
    let _wrap = under_outer(&mut ui, UVec2::new(400, 400), |ui| {
        Panel::wrap_hstack_with_id("w")
            .size((Sizing::Fixed(200.0), Sizing::Hug))
            .gap(10.0)
            .line_gap(8.0)
            .show(ui, |ui| {
                kids.push(cell(ui, "a", 60.0, 20.0));
                kids.push(cell(ui, "b", 60.0, 20.0));
                kids.push(cell(ui, "c", 60.0, 20.0));
                kids.push(cell(ui, "d", 60.0, 20.0));
            })
            .node
    });
    let a = ui.layout_engine.rect(kids[0]);
    let b = ui.layout_engine.rect(kids[1]);
    let c = ui.layout_engine.rect(kids[2]);
    let d = ui.layout_engine.rect(kids[3]);
    assert_eq!((a.min.x, a.min.y), (0.0, 0.0));
    assert_eq!((b.min.x, b.min.y), (70.0, 0.0));
    assert_eq!((c.min.x, c.min.y), (140.0, 0.0));
    assert_eq!((d.min.x, d.min.y), (0.0, 28.0));
}

/// Pin: when a child is wider than the available main, it sits alone on
/// its line (no infinite recursion, no wrapping inside the child).
#[test]
fn wrap_hstack_oversize_child_owns_its_line() {
    let mut ui = Ui::new();
    let mut kids = Vec::new();
    let _wrap = under_outer(&mut ui, UVec2::new(400, 400), |ui| {
        Panel::wrap_hstack_with_id("w")
            .size((Sizing::Fixed(100.0), Sizing::Hug))
            .gap(10.0)
            .line_gap(8.0)
            .show(ui, |ui| {
                kids.push(cell(ui, "small", 50.0, 20.0));
                kids.push(cell(ui, "wide", 200.0, 20.0));
                kids.push(cell(ui, "tail", 50.0, 20.0));
            })
            .node
    });
    let small = ui.layout_engine.rect(kids[0]);
    let wide = ui.layout_engine.rect(kids[1]);
    let tail = ui.layout_engine.rect(kids[2]);
    // line 0: small alone (50+10+200 > 100, wide overflows → wraps)
    assert_eq!((small.min.x, small.min.y), (0.0, 0.0));
    // line 1: wide alone (overflowed)
    assert_eq!((wide.min.x, wide.min.y), (0.0, 28.0));
    // line 2: tail
    assert_eq!((tail.min.x, tail.min.y), (0.0, 56.0));
}

/// Pin: line height = max child cross within the line; subsequent
/// lines start at the previous line's bottom + `line_gap`.
#[test]
fn wrap_hstack_line_height_is_max_child_cross() {
    let mut ui = Ui::new();
    let mut kids = Vec::new();
    let _wrap = under_outer(&mut ui, UVec2::new(400, 400), |ui| {
        Panel::wrap_hstack_with_id("w")
            .size((Sizing::Fixed(200.0), Sizing::Hug))
            .gap(0.0)
            .line_gap(0.0)
            .show(ui, |ui| {
                kids.push(cell(ui, "tall", 100.0, 60.0));
                kids.push(cell(ui, "short", 100.0, 20.0));
                // overflow → new line
                kids.push(cell(ui, "next-line", 100.0, 30.0));
            })
            .node
    });
    let tall = ui.layout_engine.rect(kids[0]);
    let short = ui.layout_engine.rect(kids[1]);
    let next = ui.layout_engine.rect(kids[2]);
    assert_eq!(tall.min.y, 0.0);
    assert_eq!(short.min.y, 0.0);
    // Line 0 height = 60; line_gap = 0 → next at y=60.
    assert_eq!(next.min.y, 60.0);
}

/// Pin: `Justify::Center` per-line. With a 200-wide WrapHStack and line
/// content 60+10+60 = 130, leftover = 70 → start_offset = 35.
#[test]
fn wrap_hstack_justify_center_per_line() {
    let mut ui = Ui::new();
    let mut kids = Vec::new();
    let _wrap = under_outer(&mut ui, UVec2::new(400, 400), |ui| {
        Panel::wrap_hstack_with_id("w")
            .size((Sizing::Fixed(200.0), Sizing::Hug))
            .gap(10.0)
            .line_gap(0.0)
            .justify(Justify::Center)
            .show(ui, |ui| {
                kids.push(cell(ui, "a", 60.0, 20.0));
                kids.push(cell(ui, "b", 60.0, 20.0));
            })
            .node
    });
    let a = ui.layout_engine.rect(kids[0]);
    let b = ui.layout_engine.rect(kids[1]);
    assert_eq!(a.min.x, 35.0);
    assert_eq!(b.min.x, 105.0);
}

/// Pin: WrapVStack — same code via `Axis::Y`. Children flow top-to-
/// bottom, wrap to new column on the right.
#[test]
fn wrap_vstack_wraps_columns_when_main_overflows() {
    let mut ui = Ui::new();
    let mut kids = Vec::new();
    let _wrap = under_outer(&mut ui, UVec2::new(400, 400), |ui| {
        Panel::wrap_vstack_with_id("w")
            .size((Sizing::Hug, Sizing::Fixed(100.0)))
            .gap(10.0)
            .line_gap(8.0)
            .show(ui, |ui| {
                kids.push(cell(ui, "a", 20.0, 40.0));
                kids.push(cell(ui, "b", 20.0, 40.0));
                // 40+10+40+10+40 = 140 > 100 → c wraps
                kids.push(cell(ui, "c", 20.0, 40.0));
            })
            .node
    });
    let a = ui.layout_engine.rect(kids[0]);
    let b = ui.layout_engine.rect(kids[1]);
    let c = ui.layout_engine.rect(kids[2]);
    // Column 0: a, b at x=0.
    assert_eq!((a.min.x, a.min.y), (0.0, 0.0));
    assert_eq!((b.min.x, b.min.y), (0.0, 50.0));
    // Column 1: c at x=20+8=28, y=0.
    assert_eq!((c.min.x, c.min.y), (28.0, 0.0));
}

/// Pin: a Fixed-width Hug-height WrapHStack hugs to its packed cross
/// extent. 4 cells of 60×20 in a 200-wide wrap: 3 fit on line 0
/// (60+10+60+10+60 = 190), 1 wraps to line 1. Outer h = 20+8+20 = 48.
///
/// Note: a fully-Hug WrapHStack (no main constraint anywhere up the
/// chain) collapses to a single line — intrinsic measure runs at
/// `INF` main with no width to wrap against. To force wrap, the
/// WrapHStack (or some ancestor) must commit a finite main size.
#[test]
fn wrap_hstack_with_fixed_main_hugs_cross_to_packed_lines() {
    let mut ui = Ui::new();
    let mut wrap_node = None;
    let _wrap = under_outer(&mut ui, UVec2::new(400, 400), |ui| {
        wrap_node = Some(
            Panel::wrap_hstack_with_id("w")
                .size((Sizing::Fixed(200.0), Sizing::Hug))
                .gap(10.0)
                .line_gap(8.0)
                .show(ui, |ui| {
                    cell(ui, "a", 60.0, 20.0);
                    cell(ui, "b", 60.0, 20.0);
                    cell(ui, "c", 60.0, 20.0);
                    cell(ui, "d", 60.0, 20.0);
                })
                .node,
        );
        wrap_node.unwrap()
    });
    let r = ui.layout_engine.rect(wrap_node.unwrap());
    assert_eq!(r.size.w, 200.0, "Fixed main width is honored");
    // Two lines of 20 + 8 line_gap = 48.
    assert_eq!(r.size.h, 48.0);
}

/// Pin: `Justify::SpaceBetween` per row distributes leftover as extra
/// gap between siblings on each line. 200-wide WrapHStack, two 60-wide
/// children, gap=10 → leftover=70, 1 between-gap, eff_gap = 10+70 = 80.
#[test]
fn wrap_hstack_justify_space_between_per_line() {
    let mut ui = Ui::new();
    let mut kids = Vec::new();
    let _ = under_outer(&mut ui, UVec2::new(400, 400), |ui| {
        Panel::wrap_hstack_with_id("w")
            .size((Sizing::Fixed(200.0), Sizing::Hug))
            .gap(10.0)
            .justify(Justify::SpaceBetween)
            .show(ui, |ui| {
                kids.push(cell(ui, "a", 60.0, 20.0));
                kids.push(cell(ui, "b", 60.0, 20.0));
            })
            .node
    });
    let a = ui.layout_engine.rect(kids[0]);
    let b = ui.layout_engine.rect(kids[1]);
    assert_eq!(a.min.x, 0.0);
    // 200 - 60 = 140 → b at 140, exact end-edge.
    assert_eq!(b.min.x, 140.0);
}

/// Pin: `Justify::SpaceAround` per row distributes leftover as half
/// extra padding at line edges + full between siblings. 200-wide
/// WrapHStack, two 60-wide, gap=10 → leftover=70, extra/count = 35,
/// half=17.5 leading, full=35 between → siblings at gap=10+35=45.
#[test]
fn wrap_hstack_justify_space_around_per_line() {
    let mut ui = Ui::new();
    let mut kids = Vec::new();
    let _ = under_outer(&mut ui, UVec2::new(400, 400), |ui| {
        Panel::wrap_hstack_with_id("w")
            .size((Sizing::Fixed(200.0), Sizing::Hug))
            .gap(10.0)
            .justify(Justify::SpaceAround)
            .show(ui, |ui| {
                kids.push(cell(ui, "a", 60.0, 20.0));
                kids.push(cell(ui, "b", 60.0, 20.0));
            })
            .node
    });
    let a = ui.layout_engine.rect(kids[0]);
    let b = ui.layout_engine.rect(kids[1]);
    // start_offset = 17.5; b = 17.5 + 60 + 45 = 122.5
    assert!((a.min.x - 17.5).abs() < 0.5);
    assert!((b.min.x - 122.5).abs() < 0.5);
}

/// Pin: cross-axis `Sizing::Fill` stretches to the row's tallest-child
/// height (CSS `align-items: stretch` default). Mirrors Stack cross.
#[test]
fn wrap_hstack_cross_fill_child_stretches_to_row_height() {
    let mut ui = Ui::new();
    let mut kids = Vec::new();
    let _ = under_outer(&mut ui, UVec2::new(400, 400), |ui| {
        Panel::wrap_hstack_with_id("w")
            .size((Sizing::Fixed(300.0), Sizing::Hug))
            .gap(10.0)
            .show(ui, |ui| {
                // Tall child sets the row height = 60.
                kids.push(cell(ui, "tall", 100.0, 60.0));
                // Fill-on-cross child should stretch to 60 (not stay at its
                // intrinsic).
                kids.push(
                    Frame::with_id("filler")
                        .size((Sizing::Fixed(100.0), Sizing::FILL))
                        .fill(Color::rgb(0.5, 0.5, 0.5))
                        .show(ui)
                        .node,
                );
            })
            .node
    });
    let tall = ui.layout_engine.rect(kids[0]);
    let filler = ui.layout_engine.rect(kids[1]);
    assert_eq!(tall.size.h, 60.0);
    assert_eq!(
        filler.size.h, 60.0,
        "Fill-on-cross child stretches to row height"
    );
}

/// Pin: a collapsed child mid-pack contributes nothing — neither main
/// extent nor cross extent — and doesn't insert a between-line gap or
/// shift its siblings. The collapsed node still gets a zero-size rect
/// (anchored at the line's start) so descendant rects don't carry
/// stale values from prior frames.
#[test]
fn wrap_hstack_collapsed_child_in_pack_is_skipped() {
    let mut ui = Ui::new();
    let mut kids = Vec::new();
    let _ = under_outer(&mut ui, UVec2::new(400, 400), |ui| {
        Panel::wrap_hstack_with_id("w")
            .size((Sizing::Fixed(200.0), Sizing::Hug))
            .gap(10.0)
            .show(ui, |ui| {
                kids.push(cell(ui, "a", 60.0, 20.0));
                kids.push(
                    Frame::with_id("hidden")
                        .size((Sizing::Fixed(60.0), Sizing::Fixed(20.0)))
                        .collapsed()
                        .show(ui)
                        .node,
                );
                kids.push(cell(ui, "b", 60.0, 20.0));
            })
            .node
    });
    let a = ui.layout_engine.rect(kids[0]);
    let hidden = ui.layout_engine.rect(kids[1]);
    let b = ui.layout_engine.rect(kids[2]);
    // a at 0, b at 70 — collapsed didn't insert a gap.
    assert_eq!(a.min.x, 0.0);
    assert_eq!(b.min.x, 70.0);
    // Hidden has zero size (cleared/zeroed by the collapsed branch).
    assert_eq!((hidden.size.w, hidden.size.h), (0.0, 0.0));
}

/// Pin (today's behavior): `Sizing::Fill` on a child's main axis is
/// treated as `Hug` — measure runs at INF main and the child reports
/// its content size, no per-row leftover distribution. Future work
/// adding flex-style row-leftover distribution should update this
/// test rather than introduce the new behavior silently.
#[test]
fn wrap_hstack_fill_main_child_treated_as_hug_for_now() {
    let mut ui = Ui::new();
    let mut filler_node = None;
    let _ = under_outer(&mut ui, UVec2::new(400, 400), |ui| {
        Panel::wrap_hstack_with_id("w")
            .size((Sizing::Fixed(300.0), Sizing::Hug))
            .gap(10.0)
            .show(ui, |ui| {
                cell(ui, "fixed-a", 60.0, 20.0);
                filler_node = Some(
                    Frame::with_id("filler")
                        .size((Sizing::FILL, Sizing::Fixed(20.0)))
                        // min_size makes Fill measurable as a positive
                        // number even with no row-leftover distribution.
                        .min_size((40.0, 0.0))
                        .show(ui)
                        .node,
                );
            })
            .node
    });
    let r = ui.layout_engine.rect(filler_node.unwrap());
    // Fill child got its min_size width (40), NOT the row leftover
    // (300 - 60 - 10 - 10 = 220). If a future change distributes
    // leftover, this assertion flips and the test becomes the spec.
    assert!(
        r.size.w < 100.0,
        "Fill main treated as Hug today; got w={}",
        r.size.w
    );
}

/// Pin: nested WrapStacks don't trample each other's per-line
/// scratch buffer. `LayoutEngine.wrap` is depth-stacked so the inner
/// arrange takes a different slot than the outer.
#[test]
fn nested_wrap_hstacks_do_not_trample_scratch() {
    let mut ui = Ui::new();
    let mut inner_a = None;
    let mut inner_b = None;
    let mut outer_b = None;
    let _ = under_outer(&mut ui, UVec2::new(600, 400), |ui| {
        Panel::wrap_hstack_with_id("outer")
            .size((Sizing::Fixed(500.0), Sizing::Hug))
            .gap(10.0)
            .line_gap(10.0)
            .show(ui, |ui| {
                // First outer-row child: an inner WrapHStack with two
                // cells.
                Panel::wrap_hstack_with_id("inner-card")
                    .size((Sizing::Fixed(120.0), Sizing::Hug))
                    .gap(5.0)
                    .show(ui, |ui| {
                        inner_a = Some(cell(ui, "ia", 50.0, 20.0));
                        inner_b = Some(cell(ui, "ib", 50.0, 20.0));
                    });
                outer_b = Some(cell(ui, "ob", 100.0, 20.0));
            })
            .node
    });
    let ia = ui.layout_engine.rect(inner_a.unwrap());
    let ib = ui.layout_engine.rect(inner_b.unwrap());
    let ob = ui.layout_engine.rect(outer_b.unwrap());
    // Inner card lays out two cells side by side: ia at 0, ib at 55.
    assert_eq!(ia.min.x, 0.0);
    assert_eq!(ib.min.x, 55.0);
    assert_eq!(ia.min.y, ib.min.y, "inner cells share a row");
    // Outer's second child is the cell `ob` placed after the inner
    // card — outer hasn't lost track of "we have one child so far"
    // due to the inner's scratch use.
    let inner_card_w = 120.0;
    assert_eq!(ob.min.x, inner_card_w + 10.0); // outer gap=10
}
