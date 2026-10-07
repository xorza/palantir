//! The main bound a wrap inherits from its parent, and never overflowing it.

use crate::Ui;
use crate::internals::harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::scene::layer::Layer;
use crate::scene::tree::node_id::NodeId;
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, panel::Panel};
use glam::{UVec2, Vec2};

/// Pin: a `Sizing::FILL` WrapHStack of Hug `Button`s under a padded FILL panel keeps every button within the arranged width, wrapping instead of passing the edge.
#[test]
fn wrap_hstack_buttons_never_overflow_parent_at_narrow_widths() {
    use crate::widgets::button::Button;

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

    // Returns the wrapstack's node: it is `auto_id`'d, so it has no stable `WidgetId`.
    fn build(ui: &mut Ui) -> NodeId {
        let mut wrap_node = None;
        Panel::vstack()
            .auto_id()
            .padding(12.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                wrap_node = Some(
                    Panel::wrap_hstack()
                        .auto_id()
                        .gap(6.0)
                        .line_gap(6.0)
                        .size((Sizing::FILL, Sizing::HUG))
                        .show(ui, |ui| {
                            for label in LABELS {
                                Button::new()
                                    .id(WidgetId::from_hash(label))
                                    .label(label)
                                    .show(ui);
                            }
                        })
                        .response
                        .node(),
                );
            });
        wrap_node.unwrap()
    }

    for surface_w in [800u32, 600, 500, 400, 350, 300, 250, 200, 150, 120] {
        let mut h = UiHarness::new(UVec2::new(surface_w, 600));
        let wrap = h.frame_value(build);
        let wrap_rect = h.ui.arranged_rect(Layer::Main, wrap);
        let wrap_right = wrap_rect.min.x + wrap_rect.size.w;
        for label in LABELS {
            let r = h.arranged(WidgetId::from_hash(label));
            let right = r.min.x + r.size.w;
            assert!(
                right <= wrap_right + 0.5,
                "button overflows wrapstack at surface_w={surface_w}: \
               wrap_right={wrap_right} button_right={right} (rect={r:?})",
            );
        }
    }
}

/// A `wrap_vstack` of five 50×40 cells, gap 10, line gap 12, optionally capped: against 100 px a column fits two cells (90).
fn func_wrap(ui: &mut Ui, cap: Option<f32>) {
    let mut wrap = Panel::wrap_vstack()
        .id(WidgetId::from_hash("wrap"))
        .size((Sizing::HUG, Sizing::HUG))
        .gap(10.0)
        .line_gap(12.0);
    if let Some(cap) = cap {
        wrap = wrap.max_size((f32::INFINITY, cap));
    }
    wrap.show(ui, |ui| {
        for i in 0..5u32 {
            Block::new()
                .id(WidgetId::from_hash(("f", i)))
                .size((Sizing::fixed(50.0), Sizing::fixed(40.0)))
                .show(ui);
        }
    });
}

fn hug_vstack(name: &str) -> Panel {
    Panel::vstack()
        .id(WidgetId::from_hash(name))
        .size((Sizing::HUG, Sizing::HUG))
}

/// A same-axis stack measures a `wrap_vstack` with `INF` main, so a finite bound must come from somewhere; each case puts the 100 px cap elsewhere:
/// - on the wrap itself: `AxisSlot::resolve` clamps the `INF`;
/// - on the parent vstack: a stack forwards its finite main extent to a same-axis wrap;
/// - on an hstack of `[60×15 header, wrap]` columns: the column forwards its whole 100 px, so the wrap ends at 15 + 90 = 105, past the cap;
/// - on a vstack popup above that hstack: the cap reaches the wrap through the non-wrap hstack (CSS `max-height`).
#[test]
fn wrap_vstack_wraps_against_a_main_bound_wherever_it_lives() {
    #[derive(Debug)]
    struct Case {
        name: &'static str,
        build: fn(&mut Ui),
        header: f32,
    }
    let cases = [
        Case {
            name: "cap on the wrap",
            build: |ui| {
                hug_vstack("col").show(ui, |ui| func_wrap(ui, Some(100.0)));
            },
            header: 0.0,
        },
        Case {
            name: "cap on the parent vstack",
            build: |ui| {
                hug_vstack("col")
                    .max_size((f32::INFINITY, 100.0))
                    .show(ui, |ui| func_wrap(ui, None));
            },
            header: 0.0,
        },
        Case {
            name: "cap on an hstack of columns",
            build: |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("cols"))
                    .size((Sizing::HUG, Sizing::HUG))
                    .max_size((f32::INFINITY, 100.0))
                    .show(ui, |ui| {
                        hug_vstack("cat").show(ui, |ui| {
                            Block::new()
                                .id(WidgetId::from_hash("hdr"))
                                .size((Sizing::fixed(60.0), Sizing::fixed(15.0)))
                                .show(ui);
                            func_wrap(ui, None);
                        });
                    });
            },
            header: 15.0,
        },
        Case {
            name: "cap on a vstack above the hstack",
            build: |ui| {
                hug_vstack("popup")
                    .max_size((f32::INFINITY, 100.0))
                    .show(ui, |ui| {
                        Panel::hstack()
                            .id(WidgetId::from_hash("cols"))
                            .size((Sizing::HUG, Sizing::HUG))
                            .show(ui, |ui| {
                                hug_vstack("cat").show(ui, |ui| func_wrap(ui, None));
                            });
                    });
            },
            header: 0.0,
        },
    ];
    for case in cases {
        let mut h = UiHarness::new(UVec2::new(800, 600));
        h.frame(case.build);
        for i in 0..5u32 {
            let column = (i / 2) as f32;
            let row = (i % 2) as f32;
            assert_eq!(
                h.arranged(WidgetId::from_hash(("f", i))).min,
                Vec2::new(column * 62.0, case.header + row * 50.0),
                "{}: cell {i}",
                case.name,
            );
        }
        assert_eq!(
            h.arranged(WidgetId::from_hash("wrap")).max().y,
            case.header + 90.0,
            "{}",
            case.name,
        );
    }
}

/// A stack measures every non-`Fill` child against its whole main extent and shrinks none, so the Hug wrap under a 15 px header overflows the cap by 15. A `Fill` wrap gets the 85 px left: two cells no longer fit (90 > 85), so each takes a column and the wrap ends at 55.
#[test]
fn a_fill_wrap_under_a_header_wraps_against_what_is_left() {
    let mut h = UiHarness::new(UVec2::new(800, 600));
    h.frame(|ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("cols"))
            .size((Sizing::HUG, Sizing::HUG))
            .max_size((f32::INFINITY, 100.0))
            .show(ui, |ui| {
                hug_vstack("cat").show(ui, |ui| {
                    Block::new()
                        .id(WidgetId::from_hash("hdr"))
                        .size((Sizing::fixed(60.0), Sizing::fixed(15.0)))
                        .show(ui);
                    Panel::wrap_vstack()
                        .id(WidgetId::from_hash("wrap"))
                        .size((Sizing::HUG, Sizing::FILL))
                        .gap(10.0)
                        .line_gap(12.0)
                        .show(ui, |ui| {
                            for i in 0..5u32 {
                                Block::new()
                                    .id(WidgetId::from_hash(("f", i)))
                                    .size((Sizing::fixed(50.0), Sizing::fixed(40.0)))
                                    .show(ui);
                            }
                        });
                });
            });
    });
    for i in 0..5u32 {
        assert_eq!(
            h.arranged(WidgetId::from_hash(("f", i))).min,
            Vec2::new(i as f32 * 62.0, 15.0),
            "cell {i}"
        );
    }
    assert_eq!(h.arranged(WidgetId::from_hash("wrap")).max().y, 55.0);
}

/// A Hug wrap offered less height than its lines keeps every line (floor = tallest child per line plus line gaps) and overflows its parent. Five 40 × 20 blocks, 10 px gap, 100 px width: lines of 2, 2, 1, so 3 × 20 + 2 × 5 = 70 in a 30 px parent.
#[test]
fn a_hug_wrap_stack_keeps_its_lines_under_a_short_parent() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    h.frame(|ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::fixed(100.0), Sizing::fixed(30.0)))
            .show(ui, |ui| {
                Panel::wrap_hstack()
                    .id(WidgetId::from_hash("wrap"))
                    .gap(10.0)
                    .line_gap(5.0)
                    .size((Sizing::FILL, Sizing::HUG))
                    .show(ui, |ui| {
                        for i in 0..5u32 {
                            Block::new()
                                .id(WidgetId::from_hash(("item", i)))
                                .size((Sizing::fixed(40.0), Sizing::fixed(20.0)))
                                .show(ui);
                        }
                    });
            });
    });
    assert_eq!(h.arranged(WidgetId::from_hash("wrap")).size.h, 70.0);
    assert_eq!(
        h.arranged(WidgetId::from_hash(("item", 4u32))).min.y,
        50.0,
        "the third line starts below two lines and two gaps"
    );
}
