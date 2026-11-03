//! How a stack shares its main axis between children that do not all fit: each gives way from what it wants toward its floor.

use crate::internals::layout_rig::{GridTracks, LayoutRig, NodeRecorder, TextRun};
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::scroll_axes::ScrollAxes;
use crate::primitives::layout::sizing::{SizeSpec, Sizing};
use crate::primitives::layout::track::Track;
use crate::text::wrap::TextWrap;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;

const SURFACE: Size = Size::new(800.0, 600.0);

/// One cell of a Hug column and a Hug row.
const ONE_HUG_CELL: GridTracks<'static> = GridTracks {
    rows: &[Track::HUG],
    cols: &[Track::HUG],
};

fn header(rec: &mut NodeRecorder<'_>, name: &str) {
    rec.leaf(name, Widget::leaf().size((120.0, 30.0)));
}

/// A vertical scroll over eight 50 px rows: wants 400, floor on the panned axis zero.
fn scroll(rec: &mut NodeRecorder<'_>, name: &str, pan: Sizing) {
    scroll_rows(rec, name, pan, 8);
}

/// A vertical scroll over `rows` 50 px rows, fitting them on a `Hug` pan axis.
fn scroll_rows(rec: &mut NodeRecorder<'_>, name: &str, pan: Sizing, rows: u32) {
    let size = SizeSpec::new(Sizing::HUG, pan);
    let viewport = Widget::scroll(ScrollAxes::VERTICAL.fit_content(size)).size(size);
    rec.node(name, viewport, |rec| {
        for i in 0..rows {
            rec.leaf(&format!("{name}{i}"), Widget::leaf().size((120.0, 50.0)));
        }
    });
}

/// A fixed 200 × 100 vstack around `body`.
fn bounded(rec: &mut NodeRecorder<'_>, body: impl FnOnce(&mut NodeRecorder<'_>)) {
    let column = Widget::vstack().size((Sizing::fixed(200.0), Sizing::fixed(100.0)));
    rec.node("bounded", column, body);
}

/// A leaf of one line of `chars` × `x` in the 14 px mono face, 7 px a character, wrapped as
/// `wrap`.
fn line(rec: &mut NodeRecorder<'_>, name: &str, wrap: TextWrap, chars: usize) {
    rec.text_leaf(
        name,
        Widget::leaf(),
        TextRun::new(&"x".repeat(chars), 14.0, wrap),
    );
}

/// A 30 px header above a scroll, in a middle vstack, in 100 px: the scroll gives way to the 70 its sibling leaves whether the middle hugs or fills. A Fill scroll in a Hug middle stays empty: a Fill child reports no content.
#[test]
fn a_scroll_gives_way_to_its_rigid_sibling() {
    // (label, middle, scroll, middle height, scroll height)
    let cases = [
        ("hug, hug", Sizing::HUG, Sizing::HUG, 100.0, 70.0),
        ("fill, hug", Sizing::FILL, Sizing::HUG, 100.0, 70.0),
        ("fill, fill", Sizing::FILL, Sizing::FILL, 100.0, 70.0),
        ("hug, fill", Sizing::HUG, Sizing::FILL, 30.0, 0.0),
    ];
    for (label, middle, pan, middle_h, scroll_h) in cases {
        let mut rig = LayoutRig::new();
        rig.run(SURFACE, |rec| {
            bounded(rec, |rec| {
                rec.node(
                    "middle",
                    Widget::vstack().size((Sizing::HUG, middle)),
                    |rec| {
                        header(rec, "header");
                        scroll(rec, "scroll", pan);
                    },
                );
            });
        });
        assert_eq!(rig.rect("middle").size.h, middle_h, "{label}");
        let scroll = rig.rect("scroll");
        assert_eq!((scroll.min.y, scroll.size.h), (30.0, scroll_h), "{label}");
    }
}

/// Each Hug stack in a chain gives its header 30 px of what its parent placed it at: 100, 70, 40, leaving 10 for the scroll.
#[test]
fn nested_stacks_share_down_the_chain() {
    let mut rig = LayoutRig::new();
    rig.run(SURFACE, |rec| {
        bounded(rec, |rec| {
            rec.node("a", Widget::vstack(), |rec| {
                header(rec, "header-a");
                rec.node("b", Widget::vstack(), |rec| {
                    header(rec, "header-b");
                    rec.node("c", Widget::vstack(), |rec| {
                        header(rec, "header-c");
                        scroll(rec, "scroll", Sizing::HUG);
                    });
                });
            });
        });
    });
    let heights = ["a", "b", "c", "scroll"].map(|name| rig.rect(name).size.h);
    assert_eq!(heights, [100.0, 70.0, 40.0, 10.0]);
    assert_eq!(rig.rect("scroll").min.y, 90.0);
}

/// Children that can both give way share the slack in proportion to how far each can give: in 150 px, scrolls wanting 100 and 150 split 60 and 90. Children that cannot (two fixed 60 px leaves in 100) keep their extents and overflow.
#[test]
fn siblings_give_way_in_proportion_to_what_they_can_give() {
    let mut rig = LayoutRig::new();
    rig.run(SURFACE, |rec| {
        let column = Widget::vstack().size((Sizing::fixed(200.0), Sizing::fixed(150.0)));
        rec.node("column", column, |rec| {
            scroll_rows(rec, "short", Sizing::HUG, 2);
            scroll_rows(rec, "long", Sizing::HUG, 8);
        });
    });
    assert_eq!(rig.rect("short").size.h, 60.0);
    assert_eq!(rig.rect("long").size.h, 90.0);

    let mut rig = LayoutRig::new();
    rig.run(SURFACE, |rec| {
        bounded(rec, |rec| {
            for name in ["first", "second"] {
                rec.leaf(name, Widget::leaf().size((120.0, 60.0)));
            }
        });
    });
    assert_eq!(rig.rect("first").size.h, 60.0);
    assert_eq!(rig.rect("second"), Rect::new(0.0, 60.0, 120.0, 60.0));
}

/// The share reaches a scroll through any container that hands a smaller slot down (zstack, grid): the scroll ends at the 70 its header leaves.
#[test]
fn a_share_reaches_through_a_zstack_and_a_grid() {
    let mut rig = LayoutRig::new();
    rig.run(SURFACE, |rec| {
        bounded(rec, |rec| {
            header(rec, "header");
            rec.node("zstack", Widget::zstack(), |rec| {
                scroll(rec, "scroll", Sizing::HUG);
            });
        });
    });
    assert_eq!(rig.rect("zstack").size.h, 70.0);
    assert_eq!(rig.rect("scroll").size.h, 70.0);

    let mut rig = LayoutRig::new();
    rig.run(SURFACE, |rec| {
        bounded(rec, |rec| {
            header(rec, "header");
            rec.grid("grid", Widget::grid(), ONE_HUG_CELL, |rec| {
                scroll(rec, "scroll", Sizing::HUG);
            });
        });
    });
    assert_eq!(rig.rect("grid").size.h, 70.0);
    assert_eq!(rig.rect("scroll").size.h, 70.0);
}

/// Widths are shared before children measure, so text shapes at its placed width. Mono 14 px = 7 px/char: two truncating 30-char labels want 210 each and split 300 as 150 each; two wrapping labels of 40 and 20 chars split 300 as 200 : 100, wrapping to two lines each.
#[test]
fn an_hstack_shares_its_width_before_its_children_measure() {
    let pair = |wrap: TextWrap, chars: [usize; 2]| {
        let mut rig = LayoutRig::new();
        rig.run(SURFACE, |rec| {
            let row = Widget::hstack().size((Sizing::fixed(300.0), Sizing::HUG));
            rec.node("row", row, |rec| {
                line(rec, "first", wrap, chars[0]);
                line(rec, "second", wrap, chars[1]);
            });
        });
        [rig.rect("first"), rig.rect("second")]
    };

    let truncated = pair(TextWrap::Truncate, [30, 30]);
    assert_eq!(
        truncated.map(|r| (r.min.x, r.size.w)),
        [(0.0, 150.0), (150.0, 150.0)]
    );

    let one_line = pair(TextWrap::Wrap, [4, 4])[0].size.h;
    let wrapped = pair(TextWrap::Wrap, [40, 20]);
    assert_eq!(
        wrapped.map(|r| (r.min.x, r.size.w, r.size.h)),
        [(0.0, 196.0, 2.0 * one_line), (196.0, 98.0, 2.0 * one_line)],
    );
}

/// A scroll's content keeps its measured extent on the panned axis, however small the viewport: twelve-character labels stay 84 px each in a 100 px scroll, and the viewport clips. Holds for a horizontal scroll's stack and a two-way scroll's layer.
#[test]
fn scroll_content_does_not_give_way_on_its_panned_axis() {
    for (label, axes) in [
        ("horizontal", ScrollAxes::HORIZONTAL),
        ("both", ScrollAxes::BOTH),
    ] {
        let mut rig = LayoutRig::new();
        rig.run(SURFACE, |rec| {
            let column = Widget::vstack().size((Sizing::fixed(100.0), Sizing::fixed(100.0)));
            rec.node("column", column, |rec| {
                let size = SizeSpec::new(Sizing::FILL, Sizing::FILL);
                let viewport = Widget::scroll(axes.fit_content(size)).size(size);
                rec.node("scroll", viewport, |rec| {
                    rec.node("row", Widget::hstack(), |rec| {
                        for name in ["one", "two", "three"] {
                            rec.text_leaf(
                                name,
                                Widget::leaf(),
                                TextRun::new("abcdefghijkl", 14.0, TextWrap::Ellipsis),
                            );
                        }
                    });
                });
            });
        });
        let spans = ["one", "two", "three"].map(|name| {
            let r = rig.rect(name);
            (r.min.x, r.size.w)
        });
        assert_eq!(spans, [(0.0, 84.0), (84.0, 84.0), (168.0, 84.0)], "{label}");
    }
}

/// A width is not taken back at arrange: a child keeps the width its text was shaped to and overflows. A Hug grid measures a Fill-column cell at unbounded width, so ten 4-letter words shape as one 343 px line (49 chars at 7 px); in a 200 px stack the cell still takes 343 though its floor is 28.
#[test]
fn arrange_does_not_take_a_width_back() {
    let mut rig = LayoutRig::new();
    rig.run(SURFACE, |rec| {
        let column = Widget::vstack().size((Sizing::fixed(200.0), Sizing::HUG));
        rec.node("column", column, |rec| {
            let tracks = GridTracks {
                rows: &[Track::HUG],
                cols: &[Track::FILL],
            };
            rec.grid("grid", Widget::grid(), tracks, |rec| {
                rec.text_leaf(
                    "cell",
                    Widget::leaf(),
                    TextRun::new(&["word"; 10].join(" "), 14.0, TextWrap::WrapWithOverflow),
                );
            });
        });
    });
    assert_eq!(rig.rect("cell").size.w, 343.0);
}
