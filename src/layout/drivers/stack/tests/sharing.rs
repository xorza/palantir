//! How a stack shares its main axis between children that do not all fit: each gives way from what it wants toward its floor.

use crate::Ui;
use crate::internals::harness::UiHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::layout::track::Track;
use crate::text::wrap::TextWrap;
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, grid::Grid, panel::Panel, scroll::Scroll, text::Text};
use glam::UVec2;

const SURFACE: UVec2 = UVec2::new(800, 600);

fn rect(h: &UiHarness, name: &'static str) -> Rect {
    h.arranged(WidgetId::from_hash(name))
}

fn header(ui: &mut Ui, name: &'static str) {
    Block::new()
        .id(WidgetId::from_hash(name))
        .size((Sizing::fixed(120.0), Sizing::fixed(30.0)))
        .show(ui);
}

/// A vertical scroll over eight 50 px rows: wants 400, floor on the panned axis zero.
fn scroll(ui: &mut Ui, name: &'static str, pan: Sizing) {
    Scroll::vertical()
        .id(WidgetId::from_hash(name))
        .size((Sizing::HUG, pan))
        .show(ui, |ui| {
            for i in 0..8u32 {
                Block::new()
                    .id(WidgetId::from_hash((name, i)))
                    .size((Sizing::fixed(120.0), Sizing::fixed(50.0)))
                    .show(ui);
            }
        });
}

/// A fixed 100 px vstack around `body`.
fn bounded(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    Panel::vstack()
        .auto_id()
        .size((Sizing::fixed(200.0), Sizing::fixed(100.0)))
        .show(ui, body);
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
        let mut h = UiHarness::new(SURFACE);
        h.frame(|ui| {
            bounded(ui, |ui| {
                Panel::vstack()
                    .id(WidgetId::from_hash("middle"))
                    .size((Sizing::HUG, middle))
                    .show(ui, |ui| {
                        header(ui, "header");
                        scroll(ui, "scroll", pan);
                    });
            });
        });
        assert_eq!(rect(&h, "middle").size.h, middle_h, "{label}");
        let scroll = rect(&h, "scroll");
        assert_eq!((scroll.min.y, scroll.size.h), (30.0, scroll_h), "{label}");
    }
}

/// Each Hug stack in a chain gives its header 30 px of what its parent placed it at: 100, 70, 40, leaving 10 for the scroll.
#[test]
fn nested_stacks_share_down_the_chain() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        bounded(ui, |ui| {
            Panel::vstack().id(WidgetId::from_hash("a")).show(ui, |ui| {
                header(ui, "header-a");
                Panel::vstack().id(WidgetId::from_hash("b")).show(ui, |ui| {
                    header(ui, "header-b");
                    Panel::vstack().id(WidgetId::from_hash("c")).show(ui, |ui| {
                        header(ui, "header-c");
                        scroll(ui, "scroll", Sizing::HUG);
                    });
                });
            });
        });
    });
    let heights = ["a", "b", "c", "scroll"].map(|name| rect(&h, name).size.h);
    assert_eq!(heights, [100.0, 70.0, 40.0, 10.0]);
    assert_eq!(rect(&h, "scroll").min.y, 90.0);
}

/// Children that can both give way share the slack in proportion to how far each can give: in 150 px, scrolls wanting 100 and 150 split 60 and 90. Children that cannot (two fixed 60 px blocks in 100) keep their extents and overflow.
#[test]
fn siblings_give_way_in_proportion_to_what_they_can_give() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::fixed(200.0), Sizing::fixed(150.0)))
            .show(ui, |ui| {
                for (name, rows) in [("short", 2u32), ("long", 8)] {
                    Scroll::vertical()
                        .id(WidgetId::from_hash(name))
                        .show(ui, |ui| {
                            for i in 0..rows {
                                Block::new()
                                    .id(WidgetId::from_hash((name, i)))
                                    .size((Sizing::fixed(120.0), Sizing::fixed(50.0)))
                                    .show(ui);
                            }
                        });
                }
            });
    });
    assert_eq!(rect(&h, "short").size.h, 60.0);
    assert_eq!(rect(&h, "long").size.h, 90.0);

    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        bounded(ui, |ui| {
            for name in ["first", "second"] {
                Block::new()
                    .id(WidgetId::from_hash(name))
                    .size((Sizing::fixed(120.0), Sizing::fixed(60.0)))
                    .show(ui);
            }
        });
    });
    assert_eq!(rect(&h, "first").size.h, 60.0);
    assert_eq!(rect(&h, "second"), Rect::new(0.0, 60.0, 120.0, 60.0));
}

/// The share reaches a scroll through any container that hands a smaller slot down (zstack, grid): the scroll ends at the 70 its header leaves.
#[test]
fn a_share_reaches_through_a_zstack_and_a_grid() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        bounded(ui, |ui| {
            header(ui, "header");
            Panel::zstack()
                .id(WidgetId::from_hash("zstack"))
                .show(ui, |ui| scroll(ui, "scroll", Sizing::HUG));
        });
    });
    assert_eq!(rect(&h, "zstack").size.h, 70.0);
    assert_eq!(rect(&h, "scroll").size.h, 70.0);

    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        bounded(ui, |ui| {
            header(ui, "header");
            Grid::new()
                .id(WidgetId::from_hash("grid"))
                .cols([Track::HUG])
                .rows([Track::HUG])
                .show(ui, |ui| scroll(ui, "scroll", Sizing::HUG));
        });
    });
    assert_eq!(rect(&h, "grid").size.h, 70.0);
    assert_eq!(rect(&h, "scroll").size.h, 70.0);
}

/// Widths are shared before children measure, so text shapes at its placed width. Mono 14 px = 7 px/char: two truncating 30-char labels want 210 each and split 300 as 150 each; two wrapping labels of 40 and 20 chars split 300 as 200 : 100, wrapping to two lines each.
#[test]
fn an_hstack_shares_its_width_before_its_children_measure() {
    let line = |wrap: TextWrap, chars: usize| {
        move |ui: &mut Ui, name: &'static str| {
            Text::new("x".repeat(chars))
                .id(WidgetId::from_hash(name))
                .font_size(14.0)
                .text_wrap(wrap)
                .show(ui);
        }
    };
    let pair = |first: &dyn Fn(&mut Ui, &'static str), second: &dyn Fn(&mut Ui, &'static str)| {
        let mut h = UiHarness::new(SURFACE);
        h.frame(|ui| {
            Panel::hstack()
                .auto_id()
                .size((Sizing::fixed(300.0), Sizing::HUG))
                .show(ui, |ui| {
                    first(ui, "first");
                    second(ui, "second");
                });
        });
        [rect(&h, "first"), rect(&h, "second")]
    };

    let truncated = pair(&line(TextWrap::Truncate, 30), &line(TextWrap::Truncate, 30));
    assert_eq!(
        truncated.map(|r| (r.min.x, r.size.w)),
        [(0.0, 150.0), (150.0, 150.0)]
    );

    let one_line = pair(&line(TextWrap::Wrap, 4), &line(TextWrap::Wrap, 4))[0]
        .size
        .h;
    let wrapped = pair(&line(TextWrap::Wrap, 40), &line(TextWrap::Wrap, 20));
    assert_eq!(
        wrapped.map(|r| (r.min.x, r.size.w, r.size.h)),
        [(0.0, 196.0, 2.0 * one_line), (196.0, 98.0, 2.0 * one_line)],
    );
}

/// A scroll's content keeps its measured extent on the panned axis, however small the viewport: twelve-character labels stay 84 px each in a 100 px scroll, and the viewport clips. Holds for a horizontal scroll's stack and a two-way scroll's layer.
#[test]
fn scroll_content_does_not_give_way_on_its_panned_axis() {
    type Scrolling = fn() -> Scroll<'static>;

    let labels = |ui: &mut Ui| {
        for name in ["one", "two", "three"] {
            Text::new("abcdefghijkl")
                .id(WidgetId::from_hash(name))
                .font_size(14.0)
                .text_wrap(TextWrap::Ellipsis)
                .show(ui);
        }
    };
    let scrolls: [(&str, Scrolling); 2] =
        [("horizontal", Scroll::horizontal), ("both", Scroll::both)];
    for (label, scroll) in scrolls {
        let mut h = UiHarness::new(SURFACE);
        h.frame(|ui| {
            Panel::vstack()
                .auto_id()
                .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
                .show(ui, |ui| {
                    scroll()
                        .auto_id()
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui, |ui| {
                            Panel::hstack().auto_id().show(ui, labels);
                        });
                });
        });
        let spans = ["one", "two", "three"].map(|name| {
            let r = rect(&h, name);
            (r.min.x, r.size.w)
        });
        assert_eq!(spans, [(0.0, 84.0), (84.0, 84.0), (168.0, 84.0)], "{label}");
    }
}

/// A width is not taken back at arrange: a child keeps the width its text was shaped to and overflows. A Hug grid measures a Fill-column cell at unbounded width, so ten 4-letter words shape as one 343 px line (49 chars at 7 px); in a 200 px stack the cell still takes 343 though its floor is 28.
#[test]
fn arrange_does_not_take_a_width_back() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::fixed(200.0), Sizing::HUG))
            .show(ui, |ui| {
                Grid::new()
                    .auto_id()
                    .cols([Track::FILL])
                    .rows([Track::HUG])
                    .show(ui, |ui| {
                        Text::new(["word"; 10].join(" "))
                            .id(WidgetId::from_hash("cell"))
                            .font_size(14.0)
                            .text_wrap(TextWrap::WrapWithOverflow)
                            .show(ui);
                    });
            });
    });
    assert_eq!(rect(&h, "cell").size.w, 343.0);
}
