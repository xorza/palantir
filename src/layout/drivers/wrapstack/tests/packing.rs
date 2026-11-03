//! Justify within a line, and children that pack differently or not at all.

use crate::internals::layout_rig::LayoutRig;
use crate::layout::drivers::wrapstack::tests::support::{cell, outer};
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::justify::Justify;
use crate::primitives::layout::sizing::Sizing;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;

const SURFACE: Size = Size::new(400.0, 400.0);

/// Per-line justify in a 200-wide WrapHStack with two 60-wide children (gap=10): content 130, leftover 70. Center: 35, 105. SpaceBetween: the one gap absorbs 70, 0 and 140. SpaceAround: 35 per slot, half (17.5) leading, 17.5 and 122.5 (60 + (10 + 35) = 105).
#[test]
fn wrap_hstack_justify_per_line() {
    let cases: &[(&str, Justify, [f32; 2])] = &[
        ("center", Justify::Center, [35.0, 105.0]),
        ("space_between", Justify::SpaceBetween, [0.0, 140.0]),
        ("space_around", Justify::SpaceAround, [17.5, 122.5]),
    ];
    for (label, justify, expected) in cases {
        let mut rig = LayoutRig::new();
        rig.run(SURFACE, |rec| {
            outer(rec, |rec| {
                let wrap = Widget::wrap_hstack()
                    .size((Sizing::fixed(200.0), Sizing::HUG))
                    .gap(10.0)
                    .justify(*justify);
                rec.node("w", wrap, |rec| {
                    cell(rec, "a", Size::new(60.0, 20.0));
                    cell(rec, "b", Size::new(60.0, 20.0));
                });
            });
        });
        // 130 used, 70 to place; every x is a multiple of 0.5, exact in f32.
        let xs = [rig.rect("a").min.x, rig.rect("b").min.x];
        assert_eq!(xs, *expected, "case: {label}");
    }
}

/// Today's behavior: `Sizing::fill` on a child's main axis acts as `Hug` (measured at INF main, content size, no per-row leftover). Flex-style distribution should update this test, not slip in silently.
#[test]
fn wrap_hstack_fill_main_child_treated_as_hug_for_now() {
    let mut rig = LayoutRig::new();
    rig.run(SURFACE, |rec| {
        outer(rec, |rec| {
            let wrap = Widget::wrap_hstack()
                .size((Sizing::fixed(300.0), Sizing::HUG))
                .gap(10.0);
            rec.node("w", wrap, |rec| {
                cell(rec, "fixed-a", Size::new(60.0, 20.0));
                let filler = Widget::leaf()
                    .size((Sizing::FILL, Sizing::fixed(20.0)))
                    // min_size makes Fill measurable as a positive number without leftover distribution.
                    .min_size((40.0, 0.0));
                rec.leaf("filler", filler);
            });
        });
    });
    // The Fill child gets its min_size width (40), not the row leftover (300 - 60 - 10 - 10 = 220); distribution would flip this and make the test the spec.
    assert_eq!(
        rig.rect("filler").size.w,
        40.0,
        "Fill main treated as Hug today"
    );
}
