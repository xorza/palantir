//! The main bound a wrap inherits from its parent, and never overflowing it.

use crate::internals::layout_rig::{LayoutRig, NodeRecorder, TextRun};
use crate::layout::drivers::wrapstack::tests::support::cell;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::sizing::{SizeSpec, Sizing};
use crate::text::wrap::TextWrap;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;
use glam::Vec2;

/// Pin: a `Sizing::FILL` WrapHStack of Hug padded labels under a padded FILL column keeps every
/// label within the arranged width, wrapping instead of passing the edge.
#[test]
fn wrap_hstack_labels_never_overflow_parent_at_narrow_widths() {
    const LABELS: [&str; 14] = [
        "text",
        "text layouts",
        "text edit",
        "z-order",
        "panels",
        "scroll",
        "wrap",
        "alignment",
        "justify",
        "clip",
        "visibility",
        "disabled",
        "gap",
        "buttons",
    ];

    for surface_w in [
        800.0, 600.0, 500.0, 400.0, 350.0, 300.0, 250.0, 200.0, 150.0, 120.0,
    ] {
        let mut rig = LayoutRig::new();
        rig.run(Size::new(surface_w, 600.0), |rec| {
            let column = Widget::vstack().padding(12.0).size(SizeSpec::FILL);
            rec.node("column", column, |rec| {
                let wrap = Widget::wrap_hstack()
                    .gap(6.0)
                    .line_gap(6.0)
                    .size(SizeSpec::FILL_W);
                rec.node("wrap", wrap, |rec| {
                    for label in LABELS {
                        rec.text_leaf(
                            label,
                            Widget::leaf().padding((12.0, 6.0)),
                            TextRun::new(label, 16.0, TextWrap::SingleLine),
                        );
                    }
                });
            });
        });
        let wrap_right = rig.rect("wrap").max().x;
        for label in LABELS {
            let r = rig.rect(label);
            assert!(
                r.max().x <= wrap_right,
                "label overflows wrapstack at surface_w={surface_w}: \
               wrap_right={wrap_right} label_right={} (rect={r:?})",
                r.max().x,
            );
        }
    }
}

/// A `wrap_vstack` of five 50×40 cells, gap 10, line gap 12, optionally capped: against 100 px a column fits two cells (90).
fn func_wrap(rec: &mut NodeRecorder<'_>, cap: Option<f32>) {
    let mut wrap = Widget::wrap_vstack().gap(10.0).line_gap(12.0);
    if let Some(cap) = cap {
        wrap = wrap.max_size((f32::INFINITY, cap));
    }
    rec.node("wrap", wrap, |rec| {
        for i in 0..5u32 {
            cell(rec, &format!("f{i}"), Size::new(50.0, 40.0));
        }
    });
}

/// A 60 × 15 header.
fn header(rec: &mut NodeRecorder<'_>) {
    cell(rec, "hdr", Size::new(60.0, 15.0));
}

/// A same-axis stack measures a `wrap_vstack` with `INF` main, so a finite bound must come from somewhere; each case puts the 100 px cap elsewhere:
/// - on the wrap itself: `AxisSlot::resolve` clamps the `INF`;
/// - on the parent vstack: a stack forwards its finite main extent to a same-axis wrap;
/// - on an hstack of `[60×15 header, wrap]` columns: the column forwards its whole 100 px, so the wrap ends at 15 + 90 = 105, past the cap;
/// - on a vstack popup above that hstack: the cap reaches the wrap through the non-wrap hstack (CSS `max-height`).
#[test]
fn wrap_vstack_wraps_against_a_main_bound_wherever_it_lives() {
    struct Case {
        name: &'static str,
        build: fn(&mut NodeRecorder<'_>),
        header: f32,
    }
    let cases = [
        Case {
            name: "cap on the wrap",
            build: |rec| rec.node("col", Widget::vstack(), |rec| func_wrap(rec, Some(100.0))),
            header: 0.0,
        },
        Case {
            name: "cap on the parent vstack",
            build: |rec| {
                let col = Widget::vstack().max_size((f32::INFINITY, 100.0));
                rec.node("col", col, |rec| func_wrap(rec, None));
            },
            header: 0.0,
        },
        Case {
            name: "cap on an hstack of columns",
            build: |rec| {
                let cols = Widget::hstack().max_size((f32::INFINITY, 100.0));
                rec.node("cols", cols, |rec| {
                    rec.node("cat", Widget::vstack(), |rec| {
                        header(rec);
                        func_wrap(rec, None);
                    });
                });
            },
            header: 15.0,
        },
        Case {
            name: "cap on a vstack above the hstack",
            build: |rec| {
                let popup = Widget::vstack().max_size((f32::INFINITY, 100.0));
                rec.node("popup", popup, |rec| {
                    rec.node("cols", Widget::hstack(), |rec| {
                        rec.node("cat", Widget::vstack(), |rec| func_wrap(rec, None));
                    });
                });
            },
            header: 0.0,
        },
    ];
    for case in cases {
        let mut rig = LayoutRig::new();
        rig.run(Size::new(800.0, 600.0), case.build);
        for i in 0..5u32 {
            let col = (i / 2) as f32;
            let row = (i % 2) as f32;
            assert_eq!(
                rig.rect(&format!("f{i}")).min,
                Vec2::new(col * 62.0, case.header + row * 50.0),
                "{}: cell {i}",
                case.name,
            );
        }
        assert_eq!(
            rig.rect("wrap").max().y,
            case.header + 90.0,
            "{}",
            case.name
        );
    }
}

/// A stack measures every non-`Fill` child against its whole main extent and shrinks none, so the Hug wrap under a 15 px header overflows the cap by 15. A `Fill` wrap gets the 85 px left: two cells no longer fit (90 > 85), so each takes a column and the wrap ends at 55.
#[test]
fn a_fill_wrap_under_a_header_wraps_against_what_is_left() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(800.0, 600.0), |rec| {
        let cols = Widget::hstack().max_size((f32::INFINITY, 100.0));
        rec.node("cols", cols, |rec| {
            rec.node("cat", Widget::vstack(), |rec| {
                header(rec);
                let wrap = Widget::wrap_vstack()
                    .size(SizeSpec::FILL_H)
                    .gap(10.0)
                    .line_gap(12.0);
                rec.node("wrap", wrap, |rec| {
                    for i in 0..5u32 {
                        cell(rec, &format!("f{i}"), Size::new(50.0, 40.0));
                    }
                });
            });
        });
    });
    for i in 0..5u32 {
        assert_eq!(
            rig.rect(&format!("f{i}")).min,
            Vec2::new(i as f32 * 62.0, 15.0),
            "cell {i}"
        );
    }
    assert_eq!(rig.rect("wrap").max().y, 55.0);
}

/// A Hug wrap offered less height than its lines keeps every line (floor = tallest child per line plus line gaps) and overflows its parent. Five 40 × 20 blocks, 10 px gap, 100 px width: lines of 2, 2, 1, so 3 × 20 + 2 × 5 = 70 in a 30 px parent.
#[test]
fn a_hug_wrap_stack_keeps_its_lines_under_a_short_parent() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(400.0, 400.0), |rec| {
        let column = Widget::vstack().size((Sizing::fixed(100.0), Sizing::fixed(30.0)));
        rec.node("column", column, |rec| {
            let wrap = Widget::wrap_hstack()
                .gap(10.0)
                .line_gap(5.0)
                .size(SizeSpec::FILL_W);
            rec.node("wrap", wrap, |rec| {
                for i in 0..5u32 {
                    cell(rec, &format!("item{i}"), Size::new(40.0, 20.0));
                }
            });
        });
    });
    assert_eq!(rig.rect("wrap").size.h, 70.0);
    assert_eq!(
        rig.rect("item4").min.y,
        50.0,
        "the third line starts below two lines and two gaps"
    );
}
