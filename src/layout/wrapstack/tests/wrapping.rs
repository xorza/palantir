//! Where the break falls on each axis, and what an oversize child does to
//! its line.

use crate::layout::types::sizing::Sizing;
use crate::layout::wrapstack::tests::support::{cell, rect_of};
use crate::primitives::widget_id::WidgetId;
use crate::ui::harness::UiHarness;
use crate::widgets::configure::Configure;
use crate::widgets::panel::Panel;
use glam::UVec2;

/// Pin: 60×20 cells in a 200-wide WrapHStack with gap=10, line_gap=8.
/// 3 fit on one line (60+10+60+10+60 = 200, all at y=0). A 4th cell
/// (250 > 200 with gaps) wraps to line 1 at y = 20 + 8 = 28.
#[test]
fn wrap_hstack_packs_then_wraps_on_overflow() {
    type Case = (&'static str, usize, &'static [(f32, f32)]);
    let cases: &[Case] = &[
        (
            "3_fit_single_line",
            3,
            &[(0.0, 0.0), (70.0, 0.0), (140.0, 0.0)],
        ),
        (
            "4_wraps_to_second_line",
            4,
            &[(0.0, 0.0), (70.0, 0.0), (140.0, 0.0), (0.0, 28.0)],
        ),
    ];
    for (label, count, expected) in cases {
        let mut h = UiHarness::new(UVec2::new(400, 400));
        let _wrap = h.under_outer(|ui| {
            Panel::wrap_hstack()
                .id(WidgetId::from_hash("w"))
                .size((Sizing::fixed(200.0), Sizing::HUG))
                .gap(10.0)
                .line_gap(8.0)
                .show(ui, |ui| {
                    for i in 0..*count {
                        cell(ui, ["a", "b", "c", "d"][i], 60.0, 20.0);
                    }
                })
                .response
                .node()
        });
        for (i, (want_x, want_y)) in expected.iter().enumerate() {
            let r = rect_of(&h, ["a", "b", "c", "d"][i]);
            assert_eq!(
                (r.min.x, r.min.y),
                (*want_x, *want_y),
                "case: {label} child[{i}]"
            );
        }
    }
}

/// Pin: when a child is wider than the available main, it sits alone on
/// its line (no infinite recursion, no wrapping inside the child).
#[test]
fn wrap_hstack_oversize_child_owns_its_line() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    let _wrap = h.under_outer(|ui| {
        Panel::wrap_hstack()
            .id(WidgetId::from_hash("w"))
            .size((Sizing::fixed(100.0), Sizing::HUG))
            .gap(10.0)
            .line_gap(8.0)
            .show(ui, |ui| {
                cell(ui, "small", 50.0, 20.0);
                cell(ui, "wide", 200.0, 20.0);
                cell(ui, "tail", 50.0, 20.0);
            })
            .response
            .node()
    });
    let small = rect_of(&h, "small");
    let wide = rect_of(&h, "wide");
    let tail = rect_of(&h, "tail");
    // line 0: small alone (50+10+200 > 100, wide overflows → wraps)
    assert_eq!((small.min.x, small.min.y), (0.0, 0.0));
    // line 1: wide alone (overflowed)
    assert_eq!((wide.min.x, wide.min.y), (0.0, 28.0));
    // line 2: tail
    assert_eq!((tail.min.x, tail.min.y), (0.0, 56.0));
}

/// Pin: WrapVStack — same code via `Axis::Y`. Children flow top-to-
/// bottom, wrap to new column on the right.
#[test]
fn wrap_vstack_wraps_columns_when_main_overflows() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    let _wrap = h.under_outer(|ui| {
        Panel::wrap_vstack()
            .id(WidgetId::from_hash("w"))
            .size((Sizing::HUG, Sizing::fixed(100.0)))
            .gap(10.0)
            .line_gap(8.0)
            .show(ui, |ui| {
                cell(ui, "a", 20.0, 40.0);
                cell(ui, "b", 20.0, 40.0);
                // 40+10+40+10+40 = 140 > 100 → c wraps
                cell(ui, "c", 20.0, 40.0);
            })
            .response
            .node()
    });
    let a = rect_of(&h, "a");
    let b = rect_of(&h, "b");
    let c = rect_of(&h, "c");
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
    let mut h = UiHarness::new(UVec2::new(400, 400));
    let mut wrap_node = None;
    let _wrap = h.under_outer(|ui| {
        wrap_node = Some(
            Panel::wrap_hstack()
                .id(WidgetId::from_hash("w"))
                .size((Sizing::fixed(200.0), Sizing::HUG))
                .gap(10.0)
                .line_gap(8.0)
                .show(ui, |ui| {
                    cell(ui, "a", 60.0, 20.0);
                    cell(ui, "b", 60.0, 20.0);
                    cell(ui, "c", 60.0, 20.0);
                    cell(ui, "d", 60.0, 20.0);
                })
                .response
                .node(),
        );
        wrap_node.unwrap()
    });
    let r = h.layout_rect(WidgetId::from_hash("w")).expect("arranged");
    assert_eq!(r.size.w, 200.0, "Fixed main width is honored");
    // Two lines of 20 + 8 line_gap = 48.
    assert_eq!(r.size.h, 48.0);
}

/// Pin: nested WrapStacks don't trample each other's per-line
/// scratch buffer. `LayoutEngine.wrap` is depth-stacked so the inner
/// arrange takes a different slot than the outer.
#[test]
fn nested_wrap_hstacks_do_not_trample_scratch() {
    let mut h = UiHarness::new(UVec2::new(600, 400));
    let _ = h.under_outer(|ui| {
        Panel::wrap_hstack()
            .id(WidgetId::from_hash("outer"))
            .size((Sizing::fixed(500.0), Sizing::HUG))
            .gap(10.0)
            .line_gap(10.0)
            .show(ui, |ui| {
                // First outer-row child: an inner WrapHStack with two
                // cells.
                Panel::wrap_hstack()
                    .id(WidgetId::from_hash("inner-card"))
                    .size((Sizing::fixed(120.0), Sizing::HUG))
                    .gap(5.0)
                    .show(ui, |ui| {
                        cell(ui, "ia", 50.0, 20.0);
                        cell(ui, "ib", 50.0, 20.0);
                    });
                cell(ui, "ob", 100.0, 20.0);
            })
            .response
            .node()
    });
    let ia = rect_of(&h, "ia");
    let ib = rect_of(&h, "ib");
    let ob = rect_of(&h, "ob");
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
