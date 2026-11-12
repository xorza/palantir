//! `LayoutPass::measure` must not assume a node desires no more than it was offered: a
//! non-monotonic descendant (a `wrap_hstack` beside `Fill` cells that hug padded content) can
//! desire ~10 px more. Sweeps a width range: no frame panics and every toolbar label stays in
//! its toolbar.
use crate::internals::layout_rig::{LayoutRig, TextRun};
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::sizing::Sizing;
use crate::text::wrap::TextWrap;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;

const FILL: (Sizing, Sizing) = (Sizing::FILL, Sizing::FILL);

/// Two FILL/FILL cells in an HStack; the right has a Fixed(180×80) descendant
/// (min-content floor 204). FILL siblings should split by shrink budget
/// (`available - intrinsic_min`), not weight alone, so whenever available >=
/// the summed `intrinsic_min` no child extends past the HStack's right edge.
#[test]
fn fill_siblings_with_unequal_min_content_do_not_overflow_parent() {
    for outer_w in (260u16..=600).step_by(10) {
        let outer_w = f32::from(outer_w);
        let mut rig = LayoutRig::new();
        rig.run(Size::new(outer_w, 400.0), |rec| {
            rec.node("row", Widget::hstack().gap(12.0).size(FILL), |rec| {
                let cell = || Widget::vstack().size(FILL).padding(12.0);
                rec.node("left", cell(), |rec| {
                    rec.leaf("left-bg", Widget::leaf().size(FILL));
                });
                // Right: Fixed(180×80) descendant, `intrinsic_min` = 204.
                rec.node("right", cell(), |rec| {
                    rec.node("right-z", Widget::zstack().size(FILL), |rec| {
                        rec.leaf("right-bg", Widget::leaf().size(FILL));
                        rec.leaf("right-fixed", Widget::leaf().size((180.0, 80.0)));
                    });
                });
            });
        });

        let row = rig.rect("row");
        let left = rig.rect("left");
        let right = rig.rect("right");

        // The row fills the surface, so the two cells share `outer_w − 12` (the gap): an
        // even split while each half covers the right cell's 204 floor, else the right
        // cell holds 204 and the left absorbs the squeeze (CSS Flexbox). Either way they
        // end flush with the row.
        let share = (outer_w - 12.0) / 2.0;
        assert_eq!(
            right.size.w,
            share.max(204.0),
            "outer_w={outer_w}: left.w={} right.w={}",
            left.size.w,
            right.size.w,
        );
        assert_eq!(
            [row.max().x, right.max().x],
            [outer_w; 2],
            "outer_w={outer_w}: right cell must end at the row's edge",
        );
    }
}

#[test]
fn second_pass_grow_then_overshoot_does_not_panic() {
    const LABELS: &[&str] = &[
        "text",
        "text layouts",
        "text edit",
        "z-order",
        "panels",
        "scroll",
        "wrap",
        "grid",
        "sizing",
        "alignment",
        "justify",
        "clip",
        "transform",
        "visibility",
        "disabled",
        "gap",
        "spacing",
        "buttons",
    ];
    // Sweep ~620-700 wide plus a wider band in 1 px steps, on one rig so each step is a resize.
    let mut rig = LayoutRig::new();
    for w in 480u16..=900 {
        rig.run(Size::new(f32::from(w), 600.0), |rec| {
            let column = Widget::vstack().padding(12.0).gap(12.0).size(FILL);
            rec.node("column", column, |rec| {
                // Toolbar: a `wrap_hstack` wider than `w`, so row count varies with width.
                let toolbar = Widget::wrap_hstack()
                    .gap(6.0)
                    .line_gap(6.0)
                    .size((Sizing::FILL, Sizing::HUG));
                rec.node("toolbar", toolbar, |rec| {
                    for label in LABELS {
                        rec.text_leaf(
                            label,
                            Widget::leaf().padding((12.0, 6.0)),
                            TextRun::new(label, 16.0, TextWrap::SingleLine),
                        );
                    }
                });

                // Central panel: 4 padded FILL cells with varying hug widths.
                rec.node(
                    "central",
                    Widget::zstack().size(FILL).padding(16.0),
                    |rec| {
                        rec.node("cells", Widget::hstack().gap(12.0).size(FILL), |rec| {
                            for (id, content_w) in
                                [("c1", 132.0), ("c2", 60.0), ("c3", 80.0), ("c4", 100.0)]
                            {
                                rec.node(id, Widget::vstack().size(FILL).padding(12.0), |rec| {
                                    let swatch = Widget::leaf().size((content_w, 40.0));
                                    rec.leaf(&format!("{id}-swatch"), swatch);
                                });
                            }
                        });
                    },
                );
            });
        });
        // Every label sits inside its toolbar; the toolbar itself can exceed
        // the window (the cells below have a rigid 536 px floor).
        let toolbar = rig.rect("toolbar");
        for label in LABELS {
            let r = rig.rect(label);
            assert!(
                toolbar.contains_rect(r),
                "w={w}: {label} at {r:?} leaves the toolbar at {toolbar:?}",
            );
        }
    }
}
