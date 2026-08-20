//! The main bound a wrap inherits from its parent, and never overflowing
//! it.

use crate::Ui;
use crate::layout::types::sizing::Sizing;
use crate::layout::wrapstack::tests::support::rect_of;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::node::Configure;
use crate::scene::tree::node_id::NodeId;
use crate::ui::harness::UiHarness;
use crate::widgets::{frame::Frame, panel::Panel};
use glam::UVec2;

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
            let r = rect_of(&h, label);
            let right = r.min.x + r.size.w;
            assert!(
                right <= wrap_right + 0.5,
                "button overflows wrapstack at surface_w={surface_w}: \
               wrap_right={wrap_right} button_right={right} (rect={r:?})",
            );
        }
    }
}

/// A `wrap_vstack` nested inside a `vstack` (same main axis) is measured
/// with `INF` main-axis available by the parent stack, so on its own it
/// would never wrap. An explicit `max_size` height gives it a finite wrap
/// budget — `resolve_sizing` clamps the `INF` down to the cap — so the
/// children pack into columns. Drives the darkroom new-node popup, where
/// each category's function list is a capped `wrap_vstack`.
#[test]
fn wrap_vstack_wraps_under_max_size_inside_vstack() {
    let mut h = UiHarness::new(UVec2::new(400, 600));
    h.frame(|ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("col"))
            .size((Sizing::HUG, Sizing::HUG))
            .show(ui, |ui| {
                Panel::wrap_vstack()
                    .id(WidgetId::from_hash("wrap"))
                    .size((Sizing::HUG, Sizing::HUG))
                    .max_size((f32::INFINITY, 100.0))
                    .gap(10.0)
                    .line_gap(12.0)
                    .show(ui, |ui| {
                        // 50×40 cells: a 100px column fits 2 (40 + 10 + 40 = 90);
                        // the 3rd (140 > 100) wraps to the next column.
                        for i in 0..5u32 {
                            Frame::new()
                                .id(WidgetId::from_hash(("c", i)))
                                .size((Sizing::fixed(50.0), Sizing::fixed(40.0)))
                                .show(ui);
                        }
                    });
            });
    });
    let rect = |i: u32| rect_of(&h, ("c", i));
    // Column 0 holds cells 0 and 1 (x = 0); cell 2 wraps to column 1.
    assert_eq!(rect(0).min.x, 0.0);
    assert_eq!(rect(1).min.x, 0.0);
    assert_eq!(rect(1).min.y, 50.0, "second cell stacks below the first");
    assert!(
        rect(2).min.x > 0.0,
        "third cell wraps to a new column (max_size bounded the INF main-axis)",
    );
    assert_eq!(rect(2).min.y, 0.0, "the new column starts at the top");
}

/// A `wrap_vstack` with **no cap of its own** wraps against the bound of
/// an enclosing same-axis stack: the parent `vstack`'s `max_size` height
/// flows in as the wrap's measure budget, because a stack now forwards
/// its finite main extent to same-axis wrap children (instead of `INF`).
/// This is the "set the cap on the parent, the nested wrap respects it"
/// ergonomic — no per-wrap `max_size` needed.
#[test]
fn wrap_vstack_inherits_parent_stack_main_bound() {
    let mut h = UiHarness::new(UVec2::new(400, 600));
    h.frame(|ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("col"))
            .size((Sizing::HUG, Sizing::HUG))
            .max_size((f32::INFINITY, 100.0))
            .show(ui, |ui| {
                Panel::wrap_vstack()
                    .id(WidgetId::from_hash("wrap"))
                    .size((Sizing::HUG, Sizing::HUG)) // no cap of its own
                    .gap(10.0)
                    .line_gap(12.0)
                    .show(ui, |ui| {
                        // 50×40 cells: a 100px column fits 2 (40 + 10 + 40 = 90);
                        // the 3rd (140 > 100) wraps to the next column.
                        for i in 0..5u32 {
                            Frame::new()
                                .id(WidgetId::from_hash(("c", i)))
                                .size((Sizing::fixed(50.0), Sizing::fixed(40.0)))
                                .show(ui);
                        }
                    });
            });
    });
    let rect = |i: u32| rect_of(&h, ("c", i));
    assert_eq!(rect(0).min.x, 0.0);
    assert_eq!(rect(1).min.x, 0.0);
    assert!(
        rect(2).min.x > 0.0,
        "third cell wraps to a new column against the parent vstack's 100px bound",
    );
    assert_eq!(rect(2).min.y, 0.0, "the new column starts at the top");
}

/// Mirrors the darkroom new-node popup: a height-capped `hstack` of
/// category columns, each a `vstack` of `[header, func wrap_vstack]`. The
/// cap on the hstack bounds the columns' height; that flows as each
/// column vstack's available height, which it forwards into its same-axis
/// func wrap → the funcs wrap into sub-columns. (Capping the *popup*
/// VStack instead works too — see
/// `capped_vstack_bounds_wrap_through_hstack`
/// — since a bounded stack now constrains its children on the main axis.)
#[test]
fn capped_hstack_of_columns_wraps_func_lists() {
    let mut h = UiHarness::new(UVec2::new(800, 600));
    h.frame(|ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("cols"))
            .size((Sizing::HUG, Sizing::HUG))
            .max_size((f32::INFINITY, 100.0))
            .show(ui, |ui| {
                Panel::vstack()
                    .id(WidgetId::from_hash("cat"))
                    .size((Sizing::HUG, Sizing::HUG))
                    .show(ui, |ui| {
                        // Category header above the wrapping function list.
                        Frame::new()
                            .id(WidgetId::from_hash("hdr"))
                            .size((Sizing::fixed(60.0), Sizing::fixed(15.0)))
                            .show(ui);
                        Panel::wrap_vstack()
                            .id(WidgetId::from_hash("wrap"))
                            .size((Sizing::HUG, Sizing::HUG))
                            .gap(10.0)
                            .line_gap(12.0)
                            .show(ui, |ui| {
                                // 50×40 funcs: a 100px column fits 2; the 3rd wraps.
                                for i in 0..5u32 {
                                    Frame::new()
                                        .id(WidgetId::from_hash(("f", i)))
                                        .size((Sizing::fixed(50.0), Sizing::fixed(40.0)))
                                        .show(ui);
                                }
                            });
                    });
            });
    });
    let rect = |i: u32| rect_of(&h, ("f", i));
    assert_eq!(rect(0).min.x, 0.0);
    assert!(
        rect(2).min.x > 0.0,
        "func list wraps to a 2nd sub-column under the hstack's height cap",
    );
}

/// A `max_size` on a `VStack` ancestor flows through a non-wrap `hstack`
/// into a nested func wrap — CSS `max-height` behavior. This is the exact
/// darkroom new-node popup shape (the popup body is a `VStack`): the
/// vstack hands the hstack its *bounded* height (a bounded stack now
/// constrains its children on the main axis), the hstack passes it as the
/// columns' cross height, and each column vstack forwards it to its func
/// wrap. So the cap can live on the popup, not the inner columns.
#[test]
fn capped_vstack_bounds_wrap_through_hstack() {
    let mut h = UiHarness::new(UVec2::new(800, 600));
    h.frame(|ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("popup"))
            .size((Sizing::HUG, Sizing::HUG))
            .max_size((f32::INFINITY, 100.0))
            .show(ui, |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("cols"))
                    .size((Sizing::HUG, Sizing::HUG))
                    .show(ui, |ui| {
                        Panel::vstack()
                            .id(WidgetId::from_hash("cat"))
                            .size((Sizing::HUG, Sizing::HUG))
                            .show(ui, |ui| {
                                Panel::wrap_vstack()
                                    .id(WidgetId::from_hash("wrap"))
                                    .size((Sizing::HUG, Sizing::HUG))
                                    .gap(10.0)
                                    .line_gap(12.0)
                                    .show(ui, |ui| {
                                        for i in 0..5u32 {
                                            Frame::new()
                                                .id(WidgetId::from_hash(("f", i)))
                                                .size((Sizing::fixed(50.0), Sizing::fixed(40.0)))
                                                .show(ui);
                                        }
                                    });
                            });
                    });
            });
    });
    assert!(
        rect_of(&h, ("f", 2u32)).min.x > 0.0,
        "func wrap respects the popup VStack's max-height, flowed through the hstack",
    );
}
