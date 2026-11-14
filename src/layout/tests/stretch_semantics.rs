//! Pin: `Sizing::fill` is a WPF Stretch: content size at measure, fills its slot at arrange.

use crate::internals::layout_rig::{LayoutRig, TextRun};
use crate::layout::intrinsic::len_req::LenReq;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::sizing::{SizeSpec, Sizing};
use crate::text::wrap::TextWrap;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;

/// "Hi" at 16 px, 2 × 8 = 16 wide.
const HI: TextRun<'static> = TextRun::new("Hi", 16.0, TextWrap::SingleLine);

/// A Hug container holding Fill children sizes to its content, not the grandparent's allocation.
#[test]
fn hug_parent_with_fill_children_hugs_to_content() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(800.0, 600.0), |rec| {
        rec.node("hug-parent", Widget::vstack(), |rec| {
            rec.text_leaf("label", Widget::leaf().size(SizeSpec::FILL_W), HI);
        });
    });
    let content = rig.intrinsic("label", Axis::X, LenReq::MaxContent);
    assert_eq!(content, 16.0);
    assert_eq!(
        rig.rect("hug-parent").size.w,
        content,
        "Hug parent must hug to its Fill child's content, not balloon to the 800 px surface",
    );
    assert_eq!(
        rig.rect("label").size.w,
        content,
        "Fill child arranges to fill parent's inner"
    );
}

/// **Pin:** a Fill child inside a Fixed-width parent stretches to the full inner width at arrange.
#[test]
fn fill_child_stretches_to_fixed_parent() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(800.0, 600.0), |rec| {
        let column = Widget::vstack().size((Sizing::fixed(400.0), Sizing::HUG));
        rec.node("column", column, |rec| {
            rec.leaf(
                "child",
                Widget::leaf().size((Sizing::FILL, Sizing::fixed(20.0))),
            );
        });
    });
    assert_eq!(rig.rect("child").size.w, 400.0);
}

/// Two equal-weight Fill siblings in a Fixed-width HStack each get half the inner width at arrange.
#[test]
fn equal_weight_fill_siblings_split_fixed_parent_equally() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(800.0, 600.0), |rec| {
        let row = Widget::hstack().size((Sizing::fixed(400.0), Sizing::HUG));
        rec.node("row", row, |rec| {
            for id in ["a", "b"] {
                rec.leaf(id, Widget::leaf().size((Sizing::FILL, Sizing::fixed(20.0))));
            }
        });
    });
    assert_eq!([rig.rect("a").size.w, rig.rect("b").size.w], [200.0; 2]);
}

/// A Hug VStack in a Fill canvas hugs its content, even with an internal Fill row, which arranges to the hugged width.
#[test]
fn hug_node_in_canvas_fill_children_arrange_to_hug_width() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(1600.0, 800.0), |rec| {
        rec.node("canvas", Widget::canvas().size(SizeSpec::FILL), |rec| {
            rec.node("node", Widget::vstack().at((40.0, 40.0)), |rec| {
                rec.node("row", Widget::hstack().size(SizeSpec::FILL_W), |rec| {
                    rec.leaf("block", Widget::leaf().size((50.0, 20.0)));
                });
            });
        });
    });
    assert_eq!(
        rig.rect("node").size.w,
        50.0,
        "Hug node must hug to content"
    );
    assert_eq!(rig.rect("row").size.w, 50.0);
}

/// A Hug HStack with a Hug label and a Fill spacer sizes to the label only: the spacer has zero leftover.
#[test]
fn hug_hstack_with_fill_spacer_hugs_to_label() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(400.0, 100.0), |rec| {
        rec.node("root", Widget::hstack(), |rec| {
            rec.text_leaf("label", Widget::leaf(), HI);
            rec.leaf("spacer", Widget::leaf().size(SizeSpec::FILL_W));
        });
    });
    assert_eq!(rig.rect("root").size.w, 16.0);
    assert_eq!(rig.rect("label").size.w, 16.0);
    assert_eq!(rig.rect("spacer").size.w, 0.0);
}
