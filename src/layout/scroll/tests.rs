//! Driver-level tests for [`crate::layout::scroll::measure`] and [`crate::layout::scroll::arrange`]:
//! INF-axis measure, content-extent recording into the persistent
//! [`crate::layout::scroll::ScrollLayoutState`] row, and the cache-hit fallback
//! (driver doesn't fire; row keeps last frame's `content`).

use crate::Ui;
use crate::layout::scroll::ScrollLayoutState as ScrollState;
use crate::layout::types::sizing::Sizing;
use crate::primitives::size::Size;
use crate::primitives::widget_id::WidgetId;
use crate::scene::element::Configure;
use crate::scene::layer::Layer;
use crate::widgets::frame::Frame;
use crate::widgets::panel::Panel;
use crate::widgets::scroll::Scroll;
use glam::UVec2;

const SURFACE: UVec2 = UVec2::new(400, 300);

/// Read the post-frame `ScrollState` for the scroll widget at
/// `id_salt`. State is what the codebase reads at record time and is
/// the stable observation point — on measure-cache hits the driver
/// doesn't run, but the persisted row keeps last frame's value.
fn state_for(ui: &mut Ui, id_salt: &'static str) -> ScrollState {
    *ui.scroll_state(WidgetId::from_hash(id_salt).with("__viewport"))
}

/// Vertical scroll measures children with INF on Y; content extent is
/// the children's full height. State is populated post-arrange.
#[test]
fn vertical_scroll_records_content_extent() {
    let mut ui = Ui::for_test();
    ui.run_at_without_baseline(SURFACE, |ui| {
        Scroll::vertical()
            .id(WidgetId::from_hash("scroll"))
            .size((Sizing::fixed(200.0), Sizing::fixed(100.0)))
            .show(ui, |ui| {
                for i in 0..5u32 {
                    Frame::new()
                        .id(WidgetId::from_hash(("row", i)))
                        .size((Sizing::FILL, Sizing::fixed(50.0)))
                        .show(ui);
                }
            });
    });
    assert_eq!(state_for(&mut ui, "scroll").content.h, 5.0 * 50.0);
}

/// Horizontal scroll measures children with INF on X.
#[test]
fn horizontal_scroll_records_content_extent() {
    let mut ui = Ui::for_test();
    ui.run_at_without_baseline(SURFACE, |ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Scroll::horizontal()
                    .id(WidgetId::from_hash("scroll"))
                    .size((Sizing::fixed(200.0), Sizing::fixed(80.0)))
                    .gap(4.0)
                    .show(ui, |ui| {
                        for i in 0..10u32 {
                            Frame::new()
                                .id(WidgetId::from_hash(("col", i)))
                                .size((Sizing::fixed(40.0), Sizing::FILL))
                                .show(ui);
                        }
                    });
            });
    });
    let content_w = state_for(&mut ui, "scroll").content.w;
    assert!(
        content_w > 200.0,
        "content overflows the 200 viewport on X: got {}",
        content_w,
    );
}

/// Both-axis scroll measures with both axes unbounded.
#[test]
fn both_axis_scroll_records_content_extent() {
    let mut ui = Ui::for_test();
    ui.run_at_without_baseline(SURFACE, |ui| {
        Scroll::both()
            .id(WidgetId::from_hash("scroll"))
            .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("wide-tall"))
                    .size((Sizing::fixed(300.0), Sizing::fixed(250.0)))
                    .show(ui);
            });
    });
    assert_eq!(
        state_for(&mut ui, "scroll").content,
        Size::new(300.0, 250.0)
    );
}

/// `ScrollState` survives across frames — record time reads it for
/// offset clamp + reservation guess + bar geometry.
#[test]
fn state_survives_across_frames() {
    let mut ui = Ui::for_test();
    let build = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Scroll::vertical()
                    .id(WidgetId::from_hash("scroll"))
                    .size((Sizing::fixed(150.0), Sizing::fixed(100.0)))
                    .show(ui, |ui| {
                        for i in 0..4u32 {
                            Frame::new()
                                .id(WidgetId::from_hash(("row", i)))
                                .size((Sizing::FILL, Sizing::fixed(40.0)))
                                .show(ui);
                        }
                    });
            });
    };
    ui.run_at_without_baseline(SURFACE, build);
    let f1 = state_for(&mut ui, "scroll");
    ui.run_at_without_baseline(SURFACE, build);
    let f2 = state_for(&mut ui, "scroll");
    assert_eq!(f1.content, f2.content);
    assert_eq!(f1.viewport, f2.viewport);
    assert_eq!(f1.outer, f2.outer);
    assert!(f1.seen, "first frame's relayout populated state");
    assert!(f2.seen);
    // Sanity: pinned numbers.
    assert_eq!(f1.content.h, 4.0 * 40.0);
}

/// `Scroll::content_margin` doesn't fold into the recorded `content`
/// size — margin is applied at clamp time only. Bars track real
/// content; the margin acts as invisible overscroll.
#[test]
fn content_margin_leaves_content_size_unchanged() {
    let mut ui = Ui::for_test();
    ui.run_at_without_baseline(SURFACE, |ui| {
        Scroll::both()
            .id(WidgetId::from_hash("scroll"))
            .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
            .content_margin((20.0, 50.0))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("box"))
                    .size((Sizing::fixed(80.0), Sizing::fixed(160.0)))
                    .show(ui);
            });
    });
    assert_eq!(state_for(&mut ui, "scroll").content, Size::new(80.0, 160.0));
}

/// Arranged height of the scroll widget's outer wrapper (the node that
/// carries the user's `id`).
fn scroll_height(ui: &Ui, id_salt: &'static str) -> f32 {
    let node = ui.node_for_widget_id(WidgetId::from_hash(id_salt));
    ui.layout[Layer::Main].rect[node.idx()].size.h
}

/// Build a `count`-row vertical **Hug** scroll (each row 50px tall)
/// wrapped in a Hug vstack, with the given min/max heights. Returns the
/// scroll's arranged height. A Hug scroll sizes to content (the driver
/// reports content extent on Hug panned axes); the wrapper isolates the
/// assertion from how the root itself is arranged.
fn hug_scroll_height(count: u32, min_h: f32, max_h: f32) -> f32 {
    let mut ui = Ui::for_test();
    ui.run_at_without_baseline(SURFACE, |ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .size((Sizing::HUG, Sizing::HUG))
            .show(ui, |ui| {
                Scroll::vertical()
                    .id(WidgetId::from_hash("scroll"))
                    .size((Sizing::HUG, Sizing::HUG))
                    .min_size((0.0, min_h))
                    .max_size((f32::INFINITY, max_h))
                    .show(ui, |ui| {
                        for i in 0..count {
                            Frame::new()
                                .id(WidgetId::from_hash(("row", i)))
                                .size((Sizing::fixed(120.0), Sizing::fixed(50.0)))
                                .show(ui);
                        }
                    });
            });
    });
    scroll_height(&ui, "scroll")
}

/// A `Hug` scroll sizes to its content, clamped to `[min, max]` — the
/// same "size to content, then clamp" `Hug` means for every other
/// widget, rather than collapsing to zero or filling the parent. Below
/// the cap it tracks content (3 × 50 = 150); under the floor it pins to
/// `min_size` (1 × 50 floored at 120, the 400 cap left as slack).
#[test]
fn hug_scroll_clamps_viewport_to_content() {
    // (label, row_count, min_h, max_h, expected viewport height)
    let cases: &[(&str, u32, f32, f32, f32)] = &[
        ("fits_content_below_max", 3, 0.0, 400.0, 150.0),
        ("floors_at_min", 1, 120.0, 400.0, 120.0),
    ];
    for (label, count, min_h, max_h, want) in cases {
        assert_eq!(
            hug_scroll_height(*count, *min_h, *max_h),
            *want,
            "case: {label}",
        );
    }
}

/// Past the cap: 8 × 50 = 400 of content in a `Hug` scroll capped at
/// `max_size = 200`, so the viewport stops at 200 and the content
/// overflows (scrollbar engages). Content extent still records the full
/// 400 so the bar/thumb sizing is correct.
#[test]
fn hug_scroll_caps_at_max_and_scrolls() {
    let mut ui = Ui::for_test();
    ui.run_at_without_baseline(SURFACE, |ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .size((Sizing::HUG, Sizing::HUG))
            .show(ui, |ui| {
                Scroll::vertical()
                    .id(WidgetId::from_hash("scroll"))
                    .size((Sizing::HUG, Sizing::HUG))
                    .max_size((f32::INFINITY, 200.0))
                    .show(ui, |ui| {
                        for i in 0..8u32 {
                            Frame::new()
                                .id(WidgetId::from_hash(("row", i)))
                                .size((Sizing::fixed(120.0), Sizing::fixed(50.0)))
                                .show(ui);
                        }
                    });
            });
    });
    assert_eq!(scroll_height(&ui, "scroll"), 200.0, "capped at max_size");
    let st = state_for(&mut ui, "scroll");
    assert_eq!(st.content.h, 400.0, "records full content extent");
    assert!(st.overflow.1, "content past the cap overflows on Y");

    let mut ui = Ui::for_test();
    ui.run_at_without_baseline(SURFACE, |ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::fixed(200.0), Sizing::fixed(100.0)))
            .show(ui, |ui| {
                Scroll::vertical()
                    .id(WidgetId::from_hash("parent-capped-scroll"))
                    .size((Sizing::HUG, Sizing::HUG))
                    .show(ui, |ui| {
                        for i in 0..8u32 {
                            Frame::new()
                                .id(WidgetId::from_hash(("parent-capped-row", i)))
                                .size((Sizing::fixed(120.0), Sizing::fixed(50.0)))
                                .show(ui);
                        }
                    });
            });
    });
    let st = state_for(&mut ui, "parent-capped-scroll");
    assert_eq!(st.viewport.h, 100.0, "viewport follows the parent cap");
    assert_eq!(st.content.h, 400.0, "content keeps its natural extent");
    assert!(st.overflow.1, "parent-capped content overflows on Y");
}

/// Counterpart guard: a `Fill` scroll keeps the content-independent
/// viewport — it reports zero on its pan axis, so it does **not** inflate
/// a `Hug` ancestor (a Fill scroll in a Hug parent stays collapsed, the
/// parent doesn't grow to the 150px of content). This is what `Hug` opts
/// out of, and it's unchanged from before.
#[test]
fn fill_scroll_does_not_grow_hug_parent() {
    let mut ui = Ui::for_test();
    ui.run_at_without_baseline(SURFACE, |ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .size((Sizing::HUG, Sizing::HUG))
            .show(ui, |ui| {
                Scroll::vertical()
                    .id(WidgetId::from_hash("scroll"))
                    .size((Sizing::HUG, Sizing::fill(1.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .id(WidgetId::from_hash("row"))
                            .size((Sizing::fixed(120.0), Sizing::fixed(150.0)))
                            .show(ui);
                    });
            });
    });
    assert_eq!(
        scroll_height(&ui, "scroll"),
        0.0,
        "a Fill scroll reports zero pan-axis extent; the Hug parent doesn't grow",
    );
}

/// Toggling a scroll's pan-axis `Sizing` (`Hug` ⇄ `Fill`) on the **same
/// `WidgetId`** across frames busts the `MeasureCache`: the fit bits ride
/// scroll specification, which is folded into the subtree hash.
/// Frame 1 (`Hug`) fits its 150px content; frame 2 (`Fill`) collapses in
/// the `Hug` parent. Without the payload hashing, the inner viewport's
/// hash (its own `Sizing` is a constant `Fill`) wouldn't change and the
/// stale frame-1 fit measure would be served — yielding 150 in frame 2.
#[test]
fn toggling_scroll_sizing_busts_measure_cache() {
    let mut ui = Ui::for_test();
    let build = |ui: &mut Ui, pan_h: Sizing| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .size((Sizing::HUG, Sizing::HUG))
            .show(ui, |ui| {
                Scroll::vertical()
                    .id(WidgetId::from_hash("scroll"))
                    .size((Sizing::HUG, pan_h))
                    .show(ui, |ui| {
                        Frame::new()
                            .id(WidgetId::from_hash("row"))
                            .size((Sizing::fixed(120.0), Sizing::fixed(150.0)))
                            .show(ui);
                    });
            });
    };
    ui.run_at_without_baseline(SURFACE, |ui| build(ui, Sizing::HUG));
    assert_eq!(scroll_height(&ui, "scroll"), 150.0, "Hug fits its content");
    ui.run_at_without_baseline(SURFACE, |ui| build(ui, Sizing::fill(1.0)));
    assert_eq!(
        scroll_height(&ui, "scroll"),
        0.0,
        "Fill collapses in the Hug parent — the frame-1 fit measure is not served stale",
    );
}
