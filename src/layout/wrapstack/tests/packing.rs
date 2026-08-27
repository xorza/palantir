//! Justify within a line, and the children that pack differently or not at
//! all.

use crate::layout::types::{justify::Justify, sizing::Sizing};
use crate::layout::wrapstack::tests::support::{cell, rect_of};
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::configure::Configure;
use crate::ui::harness::UiHarness;
use crate::widgets::{frame::Frame, panel::Panel};
use glam::UVec2;

/// Pin: per-line justify with a 200-wide WrapHStack and two 60-wide
/// children (gap=10). Content width = 130, leftover = 70.
///   Center:       half (35) leading → 35, 105.
///   SpaceBetween: 1 between-gap absorbs all 70 extra → 0, 140.
///   SpaceAround:  35/count = 35 per slot, half (17.5) leading → 17.5,
///                 122.5 (60 + (10 + 35) gap = 105; 17.5 + 105 = 122.5).
#[test]
fn wrap_hstack_justify_per_line() {
    let cases: &[(&str, Justify, [f32; 2])] = &[
        ("center", Justify::Center, [35.0, 105.0]),
        ("space_between", Justify::SpaceBetween, [0.0, 140.0]),
        ("space_around", Justify::SpaceAround, [17.5, 122.5]),
    ];
    for (label, justify, expected) in cases {
        let mut h = UiHarness::new(UVec2::new(400, 400));
        let _wrap = h.under_outer(|ui| {
            Panel::wrap_hstack()
                .id(WidgetId::from_hash("w"))
                .size((Sizing::fixed(200.0), Sizing::HUG))
                .gap(10.0)
                .line_gap(0.0)
                .justify(*justify)
                .show(ui, |ui| {
                    cell(ui, "a", 60.0, 20.0);
                    cell(ui, "b", 60.0, 20.0);
                })
                .response
                .node()
        });
        let a = rect_of(&h, "a");
        let b = rect_of(&h, "b");
        assert!(
            (a.min.x - expected[0]).abs() < 0.5,
            "case: {label} a.x={}",
            a.min.x
        );
        assert!(
            (b.min.x - expected[1]).abs() < 0.5,
            "case: {label} b.x={}",
            b.min.x
        );
    }
}

/// Pin: a collapsed child mid-pack contributes nothing — neither main
/// extent nor cross extent — and doesn't insert a between-line gap or
/// shift its siblings. The collapsed node still gets a zero-size rect
/// (anchored at the line's start) so descendant rects don't carry
/// stale values from prior frames.
#[test]
fn wrap_hstack_collapsed_child_in_pack_is_skipped() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    let _ = h.under_outer(|ui| {
        Panel::wrap_hstack()
            .id(WidgetId::from_hash("w"))
            .size((Sizing::fixed(200.0), Sizing::HUG))
            .gap(10.0)
            .show(ui, |ui| {
                cell(ui, "a", 60.0, 20.0);
                Frame::new()
                    .id(WidgetId::from_hash("hidden"))
                    .size((Sizing::fixed(60.0), Sizing::fixed(20.0)))
                    .collapsed()
                    .show(ui);
                cell(ui, "b", 60.0, 20.0);
            })
            .response
            .node()
    });
    let a = rect_of(&h, "a");
    let hidden = rect_of(&h, "hidden");
    let b = rect_of(&h, "b");
    // a at 0, b at 70 — collapsed didn't insert a gap.
    assert_eq!(a.min.x, 0.0);
    assert_eq!(b.min.x, 70.0);
    // Hidden has zero size (cleared/zeroed by the collapsed branch).
    assert_eq!((hidden.size.w, hidden.size.h), (0.0, 0.0));
}

/// Pin (today's behavior): `Sizing::fill` on a child's main axis is
/// treated as `Hug` — measure runs at INF main and the child reports
/// its content size, no per-row leftover distribution. Future work
/// adding flex-style row-leftover distribution should update this
/// test rather than introduce the new behavior silently.
#[test]
fn wrap_hstack_fill_main_child_treated_as_hug_for_now() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    let _ = h.under_outer(|ui| {
        Panel::wrap_hstack()
            .id(WidgetId::from_hash("w"))
            .size((Sizing::fixed(300.0), Sizing::HUG))
            .gap(10.0)
            .show(ui, |ui| {
                cell(ui, "fixed-a", 60.0, 20.0);
                Frame::new()
                    .id(WidgetId::from_hash("filler"))
                    .size((Sizing::FILL, Sizing::fixed(20.0)))
                    // min_size makes Fill measurable as a positive
                    // number even with no row-leftover distribution.
                    .min_size((40.0, 0.0))
                    .show(ui);
            })
            .response
            .node()
    });
    let r = h
        .layout_rect(WidgetId::from_hash("filler"))
        .expect("arranged");
    // Fill child got its min_size width (40), NOT the row leftover
    // (300 - 60 - 10 - 10 = 220). If a future change distributes
    // leftover, this assertion flips and the test becomes the spec.
    assert!(
        r.size.w < 100.0,
        "Fill main treated as Hug today; got w={}",
        r.size.w
    );
}
