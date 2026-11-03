//! Where the break falls on each axis, and what an oversize child does to its line.

use crate::internals::layout_rig::{LayoutRig, NodeRecorder};
use crate::layout::drivers::wrapstack::tests::support::{cell, outer};
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::sizing::Sizing;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;
use glam::Vec2;

const SURFACE: Size = Size::new(400.0, 400.0);

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
        let mut rig = LayoutRig::new();
        rig.run(SURFACE, |rec| {
            outer(rec, |rec| {
                let wrap = Widget::wrap_hstack()
                    .size((Sizing::fixed(200.0), Sizing::HUG))
                    .gap(10.0)
                    .line_gap(8.0);
                rec.node("w", wrap, |rec| {
                    for id in &["a", "b", "c", "d"][..*count] {
                        cell(rec, id, Size::new(60.0, 20.0));
                    }
                });
            });
        });
        for (i, (want_x, want_y)) in expected.iter().enumerate() {
            let r = rig.rect(["a", "b", "c", "d"][i]);
            assert_eq!(
                r.min,
                Vec2::new(*want_x, *want_y),
                "case: {label} child[{i}]"
            );
        }
    }
}

/// Pin: a child wider than the available main sits alone on its line.
#[test]
fn wrap_hstack_oversize_child_owns_its_line() {
    let mut rig = LayoutRig::new();
    rig.run(SURFACE, |rec| {
        outer(rec, |rec| {
            let wrap = Widget::wrap_hstack()
                .size((Sizing::fixed(100.0), Sizing::HUG))
                .gap(10.0)
                .line_gap(8.0);
            rec.node("w", wrap, |rec| {
                cell(rec, "small", Size::new(50.0, 20.0));
                cell(rec, "wide", Size::new(200.0, 20.0));
                cell(rec, "tail", Size::new(50.0, 20.0));
            });
        });
    });
    // line 0: small alone (50+10+200 > 100, wide overflows → wraps)
    assert_eq!(rig.rect("small").min, Vec2::new(0.0, 0.0));
    assert_eq!(rig.rect("wide").min, Vec2::new(0.0, 28.0));
    assert_eq!(rig.rect("tail").min, Vec2::new(0.0, 56.0));
}

/// Pin: WrapVStack, same code via `Axis::Y`; children flow top-to-bottom and wrap to a new column.
/// `Widget::wrap_stack(Axis::Y)` is the same container.
#[test]
fn wrap_vstack_wraps_columns_when_main_overflows() {
    let makers: [fn() -> Widget; 2] = [Widget::wrap_vstack, || Widget::wrap_stack(Axis::Y)];
    for (n, make) in makers.into_iter().enumerate() {
        let mut rig = LayoutRig::new();
        rig.run(SURFACE, |rec| {
            outer(rec, |rec| {
                let wrap = make()
                    .size((Sizing::HUG, Sizing::fixed(100.0)))
                    .gap(10.0)
                    .line_gap(8.0);
                rec.node("w", wrap, |rec| {
                    // 40+10+40+10+40 = 140 > 100 → c wraps
                    for id in ["a", "b", "c"] {
                        cell(rec, id, Size::new(20.0, 40.0));
                    }
                });
            });
        });
        assert_eq!(rig.rect("a").min, Vec2::new(0.0, 0.0), "maker {n}");
        assert_eq!(rig.rect("b").min, Vec2::new(0.0, 50.0), "maker {n}");
        assert_eq!(rig.rect("c").min, Vec2::new(28.0, 0.0), "maker {n}");
    }
}

/// Pin: a Fixed-width Hug-height WrapHStack hugs to its packed cross extent: 4 cells of 60×20 in 200 → 3 on line 0, 1 on line 1, h = 20+8+20 = 48. A fully-Hug WrapHStack collapses to one line (intrinsic measure runs at `INF` main), so some ancestor must commit a finite main size.
#[test]
fn wrap_hstack_with_fixed_main_hugs_cross_to_packed_lines() {
    let mut rig = LayoutRig::new();
    rig.run(SURFACE, |rec| {
        outer(rec, |rec| {
            let wrap = Widget::wrap_hstack()
                .size((Sizing::fixed(200.0), Sizing::HUG))
                .gap(10.0)
                .line_gap(8.0);
            rec.node("w", wrap, |rec| {
                for id in ["a", "b", "c", "d"] {
                    cell(rec, id, Size::new(60.0, 20.0));
                }
            });
        });
    });
    let r = rig.rect("w");
    assert_eq!(r.size.w, 200.0, "Fixed main width is honored");
    assert_eq!(r.size.h, 48.0);
}

/// Pin: nested WrapStacks don't trample each other's per-line scratch; `LayoutEngine.wrap` is depth-stacked.
#[test]
fn nested_wrap_hstacks_do_not_trample_scratch() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(600.0, 400.0), |rec| {
        outer(rec, |rec| {
            let wrap = Widget::wrap_hstack()
                .size((Sizing::fixed(500.0), Sizing::HUG))
                .gap(10.0)
                .line_gap(10.0);
            rec.node("outer-wrap", wrap, |rec| {
                let card = Widget::wrap_hstack()
                    .size((Sizing::fixed(120.0), Sizing::HUG))
                    .gap(5.0);
                rec.node("inner-card", card, |rec| {
                    cell(rec, "ia", Size::new(50.0, 20.0));
                    cell(rec, "ib", Size::new(50.0, 20.0));
                });
                cell(rec, "ob", Size::new(100.0, 20.0));
            });
        });
    });
    let ia = rig.rect("ia");
    let ib = rig.rect("ib");
    assert_eq!(ia.min.x, 0.0);
    assert_eq!(ib.min.x, 55.0);
    assert_eq!(ia.min.y, ib.min.y, "inner cells share a row");
    // Outer's second child is placed after the inner card, so the outer kept its count despite the inner's scratch use.
    let inner_card_w = 120.0;
    assert_eq!(rig.rect("ob").min.x, inner_card_w + 10.0); // outer gap=10
}

/// A subpixel resize must not move where a line breaks, since the measure cache key cannot see it: the cold run and a warm restore from a quarter-pixel earlier must agree. Two 50.0625-wide cells total 100.125; a surface of 100.0 and one of 100.25, as scale 4 makes of 400 and 401 px, both round to a 100 px budget, so the pair fits one line. A Hug stack arranged at its own width keeps its lines too (two 100.2 px cells: 200.4 rounds to 200).
#[test]
fn a_subpixel_resize_keeps_the_break_its_cache_key_stands_for() {
    fn pair(width: f32) -> impl Fn(&mut NodeRecorder<'_>) {
        move |rec| {
            rec.node("w", Widget::wrap_hstack(), |rec| {
                cell(rec, "a", Size::new(width, 20.0));
                cell(rec, "b", Size::new(width, 20.0));
            });
        }
    }

    let mut cold = LayoutRig::new();
    cold.run(Size::new(100.25, 75.0), pair(50.0625));

    let mut warm = LayoutRig::new();
    warm.run(Size::new(100.0, 75.0), pair(50.0625));
    warm.run(Size::new(100.25, 75.0), pair(50.0625));

    assert_eq!(
        cold.rect("w").size.h,
        20.0,
        "100.125 of children rounds to a 100 px budget and fits one line",
    );
    assert_eq!(
        warm.rect("w").size.h,
        cold.rect("w").size.h,
        "a warm run must answer what a cold one answers for the same surface",
    );

    let mut rig = LayoutRig::new();
    rig.run(SURFACE, pair(100.2));
    assert_eq!(rig.rect("w").size.h, 20.0, "one line, as measured");
    assert_eq!(rig.rect("b").min.y, rig.rect("a").min.y, "b beside a");
}
