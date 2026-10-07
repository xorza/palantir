//! Where the break falls on each axis, and what an oversize child does to its line.

use crate::internals::harness::UiHarness;
use crate::layout::drivers::wrapstack::tests::support::cell;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use glam::UVec2;

/// Pin: 60×20 cells in a 200-wide WrapHStack, gap 10, line_gap 8: 3 fit (60+10+60+10+60 = 200); a 4th wraps to y = 20 + 8 = 28.
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
        h.under_outer(|ui| {
            Panel::wrap_hstack()
                .id(WidgetId::from_hash("w"))
                .size((Sizing::fixed(200.0), Sizing::HUG))
                .gap(10.0)
                .line_gap(8.0)
                .show(ui, |ui| {
                    for i in 0..*count {
                        cell(ui, ["a", "b", "c", "d"][i], 60.0, 20.0);
                    }
                });
        });
        for (i, (want_x, want_y)) in expected.iter().enumerate() {
            let r = h.arranged(WidgetId::from_hash(["a", "b", "c", "d"][i]));
            assert_eq!(
                (r.min.x, r.min.y),
                (*want_x, *want_y),
                "case: {label} child[{i}]"
            );
        }
    }
}

/// Pin: a child wider than the available main sits alone on its line.
#[test]
fn wrap_hstack_oversize_child_owns_its_line() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    h.under_outer(|ui| {
        Panel::wrap_hstack()
            .id(WidgetId::from_hash("w"))
            .size((Sizing::fixed(100.0), Sizing::HUG))
            .gap(10.0)
            .line_gap(8.0)
            .show(ui, |ui| {
                cell(ui, "small", 50.0, 20.0);
                cell(ui, "wide", 200.0, 20.0);
                cell(ui, "tail", 50.0, 20.0);
            });
    });
    let small = h.arranged(WidgetId::from_hash("small"));
    let wide = h.arranged(WidgetId::from_hash("wide"));
    let tail = h.arranged(WidgetId::from_hash("tail"));
    // line 0: small alone (50+10+200 > 100, wide overflows → wraps)
    assert_eq!((small.min.x, small.min.y), (0.0, 0.0));
    assert_eq!((wide.min.x, wide.min.y), (0.0, 28.0));
    assert_eq!((tail.min.x, tail.min.y), (0.0, 56.0));
}

/// Pin: WrapVStack, same code via `Axis::Y`; children flow top-to-bottom and wrap to a new column.
#[test]
fn wrap_vstack_wraps_columns_when_main_overflows() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    h.under_outer(|ui| {
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
            });
    });
    let a = h.arranged(WidgetId::from_hash("a"));
    let b = h.arranged(WidgetId::from_hash("b"));
    let c = h.arranged(WidgetId::from_hash("c"));
    assert_eq!((a.min.x, a.min.y), (0.0, 0.0));
    assert_eq!((b.min.x, b.min.y), (0.0, 50.0));
    assert_eq!((c.min.x, c.min.y), (28.0, 0.0));
}

/// Pin: a Fixed-width Hug-height WrapHStack hugs to its packed cross extent: 4 cells of 60×20 in 200 → 3 on line 0, 1 on line 1, h = 20+8+20 = 48. A fully-Hug WrapHStack collapses to one line (intrinsic measure runs at `INF` main), so some ancestor must commit a finite main size.
#[test]
fn wrap_hstack_with_fixed_main_hugs_cross_to_packed_lines() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    let mut wrap_node = None;
    h.under_outer(|ui| {
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
    let r = h.arranged(WidgetId::from_hash("w"));
    assert_eq!(r.size.w, 200.0, "Fixed main width is honored");
    assert_eq!(r.size.h, 48.0);
}

/// Pin: nested WrapStacks don't trample each other's per-line scratch; `LayoutEngine.wrap` is depth-stacked.
#[test]
fn nested_wrap_hstacks_do_not_trample_scratch() {
    let mut h = UiHarness::new(UVec2::new(600, 400));
    h.under_outer(|ui| {
        Panel::wrap_hstack()
            .id(WidgetId::from_hash("outer"))
            .size((Sizing::fixed(500.0), Sizing::HUG))
            .gap(10.0)
            .line_gap(10.0)
            .show(ui, |ui| {
                Panel::wrap_hstack()
                    .id(WidgetId::from_hash("inner-card"))
                    .size((Sizing::fixed(120.0), Sizing::HUG))
                    .gap(5.0)
                    .show(ui, |ui| {
                        cell(ui, "ia", 50.0, 20.0);
                        cell(ui, "ib", 50.0, 20.0);
                    });
                cell(ui, "ob", 100.0, 20.0);
            });
    });
    let ia = h.arranged(WidgetId::from_hash("ia"));
    let ib = h.arranged(WidgetId::from_hash("ib"));
    let ob = h.arranged(WidgetId::from_hash("ob"));
    assert_eq!(ia.min.x, 0.0);
    assert_eq!(ib.min.x, 55.0);
    assert_eq!(ia.min.y, ib.min.y, "inner cells share a row");
    // Outer's second child is placed after the inner card, so the outer kept its count despite the inner's scratch use.
    let inner_card_w = 120.0;
    assert_eq!(ob.min.x, inner_card_w + 10.0); // outer gap=10
}

/// A subpixel resize must not move where a line breaks, since the measure cache key cannot see it: the cold frame and a warm restore from a quarter-pixel earlier must agree. Two 50.0625-wide cells total 100.125; at scale 4 a 400 px surface is 100.0 and a 401 px one 100.25, both rounding to a 100 px budget, so the pair fits one line. A Hug stack arranged at its own width keeps its lines too (two 100.2 px cells: 200.4 rounds to 200).
#[test]
fn a_subpixel_resize_keeps_the_break_its_cache_key_stands_for() {
    fn build(ui: &mut crate::Ui) {
        Panel::wrap_hstack()
            .id(WidgetId::from_hash("w"))
            .size((Sizing::HUG, Sizing::HUG))
            .show(ui, |ui| {
                cell(ui, "a", 50.0625, 20.0);
                cell(ui, "b", 50.0625, 20.0);
            });
    }
    let height = |h: &UiHarness| h.arranged(WidgetId::from_hash("w")).size.h;

    let mut cold = UiHarness::new(UVec2::new(401, 300)).scale(4.0);
    cold.frame(build);

    let mut warm = UiHarness::new(UVec2::new(400, 300)).scale(4.0);
    warm.frame(build);
    warm.resize(UVec2::new(401, 300));
    warm.frame(build);

    assert_eq!(
        height(&cold),
        20.0,
        "100.125 of children rounds to a 100 px budget and fits one line",
    );
    assert_eq!(
        height(&warm),
        height(&cold),
        "a warm frame must answer what a cold one answers for the same surface",
    );

    let mut h = UiHarness::new(UVec2::new(400, 300));
    h.frame(|ui| {
        Panel::wrap_hstack()
            .id(WidgetId::from_hash("w"))
            .size((Sizing::HUG, Sizing::HUG))
            .show(ui, |ui| {
                cell(ui, "a", 100.2, 20.0);
                cell(ui, "b", 100.2, 20.0);
            });
    });
    assert_eq!(
        h.arranged(WidgetId::from_hash("w")).size.h,
        20.0,
        "one line, as measured"
    );
    assert_eq!(
        h.arranged(WidgetId::from_hash("b")).min.y,
        h.arranged(WidgetId::from_hash("a")).min.y,
        "b beside a"
    );
}
