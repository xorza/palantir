use crate::internals::layout_rig::LayoutRig;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::align::{Align, HAlign, VAlign};
use crate::primitives::layout::sizing::{SizeSpec, Sizing};
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;

const SURFACE: Size = Size::new(200.0, 200.0);

#[test]
fn zstack_hugs_to_largest_child_per_axis_independently() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(800.0, 600.0), |rec| {
        rec.node("panel", Widget::zstack(), |rec| {
            rec.leaf("a", Widget::leaf().size((40.0, 20.0)));
            rec.leaf("b", Widget::leaf().size((10.0, 80.0)));
        });
    });
    assert_eq!(rig.rect("panel").size, Size::new(40.0, 80.0));
}

#[test]
fn zstack_lays_children_at_inner_top_left_by_default() {
    let mut rig = LayoutRig::new();
    rig.run(SURFACE, |rec| {
        rec.node(
            "panel",
            Widget::zstack()
                .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
                .padding(8.0),
            |rec| {
                rec.leaf("a", Widget::leaf().size((20.0, 20.0)));
                rec.leaf("b", Widget::leaf().size((30.0, 30.0)));
            },
        );
    });
    assert_eq!(rig.rect("a"), Rect::new(8.0, 8.0, 20.0, 20.0));
    assert_eq!(rig.rect("b"), Rect::new(8.0, 8.0, 30.0, 30.0));
}

/// A 100×100 ZStack under a 200×200 surface: child offsets resolve per axis as child override, else parent default, else Start.
#[test]
fn zstack_per_axis_alignment() {
    type Case = (
        &'static str,
        Option<Align>,                        // parent .child_align
        Vec<((f32, f32), Align, (f32, f32))>, // (child_size, child_align, expected_offset)
    );
    let cases: Vec<Case> = vec![
        (
            "no_parent_default_two_children_full_overrides",
            None,
            vec![
                ((20.0, 20.0), Align::CENTER, (40.0, 40.0)),
                (
                    (10.0, 10.0),
                    Align::new(HAlign::Right, VAlign::Bottom),
                    (90.0, 90.0),
                ),
            ],
        ),
        (
            "parent_center_with_h_override_only",
            Some(Align::CENTER),
            // Child: 20×20, override H=Right (auto V → CENTER). Expected: x=80, y=40.
            vec![((20.0, 20.0), Align::from(HAlign::Right), (80.0, 40.0))],
        ),
    ];
    for (label, parent_align, children) in &cases {
        let mut rig = LayoutRig::new();
        rig.run(SURFACE, |rec| {
            let mut panel = Widget::zstack().size((Sizing::fixed(100.0), Sizing::fixed(100.0)));
            if let Some(a) = *parent_align {
                panel = panel.child_align(a);
            }
            rec.node("panel", panel, |rec| {
                for (i, ((w, h), align, _)) in children.iter().enumerate() {
                    rec.leaf(
                        &format!("c{i}"),
                        Widget::leaf().size((*w, *h)).align(*align),
                    );
                }
            });
        });
        let kids = rig.child_rects("panel");
        for (i, (_, _, expected)) in children.iter().enumerate() {
            assert_eq!(
                (kids[i].min.x, kids[i].min.y),
                *expected,
                "case: {label} child[{i}]"
            );
        }
    }
}

#[test]
fn zstack_fill_child_stretches_to_inner() {
    let mut rig = LayoutRig::new();
    rig.run(SURFACE, |rec| {
        rec.node(
            "panel",
            Widget::zstack()
                .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
                .padding(10.0),
            |rec| rec.leaf("filler", Widget::leaf().size(SizeSpec::FILL)),
        );
    });
    assert_eq!(rig.rect("filler"), Rect::new(10.0, 10.0, 80.0, 80.0));
}

#[test]
fn hug_zstack_with_only_fill_children_collapses_to_zero() {
    // A Fill child reports its content at measure whatever it is offered and a leaf has none, so a Hug ZStack has nothing to grow to.
    let mut rig = LayoutRig::new();
    rig.run(SURFACE, |rec| {
        rec.node("panel", Widget::zstack(), |rec| {
            rec.leaf("filler", Widget::leaf().size(SizeSpec::FILL));
        });
    });
    assert_eq!(rig.rect("panel").size, Size::ZERO);
}
