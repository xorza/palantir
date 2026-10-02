//! The main bound a wrap inherits from its parent, and never overflowing
//! it.

use crate::Ui;
use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::tree::node_id::NodeId;
use crate::ui::harness::UiHarness;
use crate::widgets::configure::Configure;
use crate::widgets::{block::Block, panel::Panel};
use glam::{UVec2, Vec2};

/// Pin issue 2: showcase tab-toolbar pattern. A `Sizing::FILL`
/// WrapHStack containing many `Button` children (each Hug-sized,
/// driven by their non-wrapping label text), nested under a FILL
/// panel with padding. Every button must fit within the wrapstack's
/// arranged width — wrapping to a new row when necessary, never
/// extending past the right edge.
#[test]
fn wrap_hstack_buttons_never_overflow_parent_at_narrow_widths() {
    use crate::widgets::button::Button;

    // Shared between the fixture and the assertions, so the two cannot
    // drift the way a parallel `Vec<NodeId>` could.
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

    // Returns the wrapstack's node: it is `auto_id`'d, so unlike the
    // buttons it has no stable `WidgetId` to read back by.
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

/// A `wrap_vstack` of five 50×40 cells, gap 10, line gap 12, capped at
/// `cap` height when given. Against a 100 px bound a column fits two cells
/// (40 + 10 + 40 = 90); the third (140 > 100) wraps to the next column,
/// 50 + 12 over.
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

/// A same-axis stack measures a `wrap_vstack` with `INF` main available,
/// so the wrap needs a finite bound from somewhere. Each case puts the
/// 100 px cap in a different place, and the darkroom new-node popup uses
/// all of them:
/// - on the wrap itself — `AxisSlot::resolve` clamps the `INF` to it;
/// - on the parent vstack — a stack forwards its finite main extent to a
///   same-axis wrap child;
/// - on an hstack of category columns, each a vstack of
///   `[60×15 header, wrap]` — the hstack's cross bound becomes each
///   column's height. The column forwards its whole 100 px, not the 85
///   left under the header, so the wrap ends at 15 + 90 = 105, past the
///   cap: the stack's overflow rule for a child measured against the full
///   bound;
/// - on a vstack popup above that hstack — a bounded stack constrains its
///   children on the main axis, so the cap reaches the wrap through the
///   non-wrap hstack (CSS `max-height`).
#[test]
fn wrap_vstack_wraps_against_a_main_bound_wherever_it_lives() {
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

/// The stack contract (REDESIGN decision D-1): a stack measures every
/// non-`Fill` child against its whole main extent and shrinks none of
/// them, so the Hug wrap under a header above overflows the cap by the
/// header's 15 px. A wrap meant to take what the header leaves is `Fill`
/// on the main axis: the column hands it the 100 − 15 = 85 px left, two
/// cells no longer fit (40 + 10 + 40 = 90 > 85), so each cell takes a
/// column of its own, 50 + 12 apart. The column hugs that one row, so the
/// wrap ends at 15 + 40 = 55, inside the cap.
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
