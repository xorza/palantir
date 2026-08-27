//! Driver-level tests for [`Scroll`](crate::layout::scroll::Scroll)'s
//! measure and arrange.

use crate::Ui;
use crate::layout::types::layout_mode::ScrollSpec;
use crate::layout::types::sizing::Sizing;
use crate::layout::types::track::Track;
use crate::primitives::size::Size;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::{Configure, Node};
use crate::ui::harness::UiHarness;
use crate::widgets::frame::Frame;
use crate::widgets::grid::Grid;
use crate::widgets::panel::Panel;
use crate::widgets::scroll::Scroll;
use glam::{BVec2, UVec2};

const SURFACE: UVec2 = UVec2::new(400, 300);

#[derive(Clone, Copy, Debug)]
struct ScrollLayoutSnapshot {
    outer: Size,
    viewport: Size,
    content: Size,
}

fn layout_for(ui: &Ui, id_salt: &'static str) -> ScrollLayoutSnapshot {
    let outer_id = WidgetId::from_hash(id_salt);
    let viewport_id = outer_id.with("viewport");
    let outer = ui
        .cascade()
        .by_id
        .get(&outer_id)
        .expect("scroll outer endpoint");
    let viewport = ui
        .cascade()
        .by_id
        .get(&viewport_id)
        .expect("scroll viewport endpoint");
    let viewport_tree = ui.tree(viewport.layer);
    let viewport_rect = ui.arranged_rect(viewport.layer, viewport.node);
    ScrollLayoutSnapshot {
        outer: ui.arranged_rect(outer.layer, outer.node).size,
        viewport: viewport_rect
            .deflated_by(viewport_tree.records.layout()[viewport.node.idx()].padding)
            .size,
        content: ui.layout(viewport.layer).scroll_content[viewport.node.idx()],
    }
}

/// Vertical scroll measures children with INF on Y; content extent is
/// the children's full height. State is populated post-arrange.
#[test]
fn vertical_scroll_records_content_extent() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
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
    assert_eq!(layout_for(&h.ui, "scroll").content.h, 5.0 * 50.0);
}

/// Horizontal scroll measures children with INF on X.
#[test]
fn horizontal_scroll_records_content_extent() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
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
    let content_w = layout_for(&h.ui, "scroll").content.w;
    assert!(
        content_w > 200.0,
        "content overflows the 200 viewport on X: got {}",
        content_w,
    );
}

/// Both-axis scroll measures with both axes unbounded.
#[test]
fn both_axis_scroll_records_content_extent() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
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
    assert_eq!(layout_for(&h.ui, "scroll").content, Size::new(300.0, 250.0));
}

/// Cached measure output restores every scroll geometry input.
#[test]
fn layout_output_survives_across_frames() {
    let mut h = UiHarness::new(SURFACE);
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
    h.frame(build);
    let f1 = layout_for(&h.ui, "scroll");
    h.frame(build);
    let f2 = layout_for(&h.ui, "scroll");
    assert_eq!(f1.content, f2.content);
    assert_eq!(f1.viewport, f2.viewport);
    assert_eq!(f1.outer, f2.outer);
    assert_eq!(f1.content.h, 4.0 * 40.0);
}

/// `Scroll::content_margin` doesn't fold into the recorded `content`
/// size — margin is applied at clamp time only. Bars track real
/// content; the margin acts as invisible overscroll.
#[test]
fn content_margin_leaves_content_size_unchanged() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
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
    assert_eq!(layout_for(&h.ui, "scroll").content, Size::new(80.0, 160.0));
}

/// Arranged height of the scroll widget's outer wrapper (the node that
/// carries the user's `id`).
fn scroll_height(h: &UiHarness, id_salt: &'static str) -> f32 {
    h.layout_rect(WidgetId::from_hash(id_salt))
        .expect("arranged")
        .size
        .h
}

/// Build a `count`-row vertical **Hug** scroll (each row 50px tall)
/// wrapped in a Hug vstack, with the given min/max heights. Returns the
/// scroll's arranged height. A Hug scroll sizes to content (the driver
/// reports content extent on Hug panned axes); the wrapper isolates the
/// assertion from how the root itself is arranged.
fn hug_scroll_height(count: u32, min_h: f32, max_h: f32) -> f32 {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
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
    scroll_height(&h, "scroll")
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
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
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
    assert_eq!(scroll_height(&h, "scroll"), 200.0, "capped at max_size");
    let st = layout_for(&h.ui, "scroll");
    assert_eq!(st.content.h, 400.0, "records full content extent");
    assert!(
        st.content.h > st.viewport.h,
        "content past the cap overflows on Y"
    );

    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
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
    let st = layout_for(&h.ui, "parent-capped-scroll");
    assert_eq!(st.viewport.h, 100.0, "viewport follows the parent cap");
    assert_eq!(st.content.h, 400.0, "content keeps its natural extent");
    assert!(
        st.content.h > st.viewport.h,
        "parent-capped content overflows on Y"
    );
}

/// Counterpart guard: a `Fill` scroll keeps the content-independent
/// viewport — it reports zero on its pan axis, so it does **not** inflate
/// a `Hug` ancestor (a Fill scroll in a Hug parent stays collapsed, the
/// parent doesn't grow to the 150px of content). This is what `Hug` opts
/// out of, and it's unchanged from before.
#[test]
fn fill_scroll_does_not_grow_hug_parent() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
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
        scroll_height(&h, "scroll"),
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
    let mut h = UiHarness::new(SURFACE);
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
    h.frame(|ui| build(ui, Sizing::HUG));
    assert_eq!(scroll_height(&h, "scroll"), 150.0, "Hug fits its content");
    h.frame(|ui| build(ui, Sizing::fill(1.0)));
    assert_eq!(
        scroll_height(&h, "scroll"),
        0.0,
        "Fill collapses in the Hug parent — the frame-1 fit measure is not served stale",
    );
}

/// Pin: a `Hug` scroll reports its content extent as its **intrinsic**,
/// not merely as its measured size.
///
/// `Scroll` sets the viewport's `fit` bit on any panned axis the author
/// left `Hug` — that is what makes a scroll size to its content. Measure
/// honoured that bit from the start; the intrinsic query did not, and
/// answered zero for every panned axis. Nothing downstream of `measure`
/// noticed, because `AxisCtx::resolve` takes `max(content,
/// intrinsic_min)` and content won. A Hug grid column is where it
/// showed: column widths come from the Phase-1 *intrinsic* walk, so the
/// column resolved to zero and the cell it was meant to size overflowed
/// it.
#[test]
fn hug_scroll_drives_the_hug_grid_column_it_sits_in() {
    const CONTENT_W: f32 = 120.0;

    let mut h = UiHarness::new(SURFACE);
    let root = h.frame_value(|ui| {
        Grid::new()
            .auto_id()
            .cols([Track::hug()])
            .rows([Track::hug()])
            .size((Sizing::HUG, Sizing::HUG))
            .show(ui, |ui| {
                Scroll::horizontal()
                    .id(WidgetId::from_hash("hug-scroll"))
                    .size((Sizing::HUG, Sizing::fixed(40.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .id(WidgetId::from_hash("wide"))
                            .size((Sizing::fixed(CONTENT_W), Sizing::fixed(20.0)))
                            .show(ui);
                    });
            })
            .response
            .node()
    });

    // The Hug column is resolved from the cell's intrinsic, so the
    // scroll's own arranged width is the column width.
    let cell = h.main_child_rects(root)[0];
    assert_eq!(
        cell.size.w, CONTENT_W,
        "a Hug column must resolve to the Hug scroll's content width, not collapse to zero",
    );
}

/// **A scroll viewport takes the slot it is placed in, whichever driver
/// places it.**
///
/// Its `desired` follows the content it scrolls — that is what lets a `Hug`
/// wrapper size to it — so a bounded parent hands one a slot smaller than
/// the desired it measured. The viewport clips, so it must not overflow that
/// slot the way an ordinary node does.
///
/// One case per placing driver, because the clamp used to live inside
/// `ZStack::arrange`: a bare scroll node in a Grid or a stack's cross axis
/// got no clamp at all, and `TextEdit` is exactly such a node. A stack's
/// *main* axis is the one placement `AxisPlacement` does not own — its flex
/// solver shrinks against the zero min-content a panned scroll reports — so
/// it is covered here too, as the same outcome by another route.
///
/// Canvas is deliberately absent: on a `Hug` axis its slot *is* the child's
/// own desired (a canvas takes its size from the children it positions, so
/// it has no independent room to pull a viewport into), and on a sized axis
/// it hands `inner`, which the measure pass already bounded the desired
/// against. The clamp is the identity either way.
#[test]
fn a_scroll_viewport_takes_its_slot_under_every_driver_that_places_one() {
    const SLOT: Size = Size { w: 200.0, h: 100.0 };
    const CONTENT: Size = Size { w: 400.0, h: 400.0 };
    const SCROLL: &str = "bare-scroll";

    /// A bare `Node::scroll`, not the `Scroll` widget: the widget wraps its
    /// viewport in a ZStack of its own, which is the one driver that always
    /// clamped. `TextEdit` records the bare form, and this is its shape.
    fn record_scroll(ui: &mut Ui) {
        // `fit` on both panned axes is what makes a `Hug` scroll report its
        // content extent — the state whose desired can outgrow the slot.
        let mut node = Node::scroll(ScrollSpec::BOTH.with_fit(BVec2::TRUE));
        node.size = Some((Sizing::HUG, Sizing::HUG).into());
        ui.widget(node.id(WidgetId::from_hash(SCROLL)))
            .record(ui, None, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("content"))
                    .size((Sizing::fixed(CONTENT.w), Sizing::fixed(CONTENT.h)))
                    .show(ui);
            });
    }

    for driver in ["zstack", "hstack", "vstack", "grid"] {
        let mut h = UiHarness::new(SURFACE);
        h.frame(|ui| {
            let parent = WidgetId::from_hash("parent");
            // **Hug capped by `max_size`, not `Fixed`.** A Hug axis measures
            // its children against `INFINITY`, so the scroll's desired is its
            // content's; the cap is then what makes the slot smaller than
            // that desired. A Fixed parent bounds the measure instead and
            // never reaches the placement this is about.
            let sized = (Sizing::HUG, Sizing::HUG);
            let cap = (SLOT.w, SLOT.h);
            match driver {
                "zstack" => {
                    Panel::zstack()
                        .id(parent)
                        .size(sized)
                        .max_size(cap)
                        .show(ui, record_scroll);
                }
                "hstack" => {
                    Panel::hstack()
                        .id(parent)
                        .size(sized)
                        .max_size(cap)
                        .show(ui, record_scroll);
                }
                "vstack" => {
                    Panel::vstack()
                        .id(parent)
                        .size(sized)
                        .max_size(cap)
                        .show(ui, record_scroll);
                }
                _ => {
                    Grid::new()
                        .id(parent)
                        .size(sized)
                        .max_size(cap)
                        .cols([Track::hug()])
                        .rows([Track::hug()])
                        .show(ui, record_scroll);
                }
            }
        });
        let scroll_id = WidgetId::from_hash(SCROLL);
        let rect =
            h.ui.response_for(scroll_id)
                .rect
                .expect("the scroll arranged");
        assert_eq!(
            (rect.size.w, rect.size.h),
            (SLOT.w, SLOT.h),
            "{driver}: a viewport measuring {CONTENT:?} must still take its {SLOT:?} slot",
        );
        assert_eq!(
            h.ui.scroll_content(scroll_id),
            CONTENT,
            "{driver}: and still record the full content extent for its bars",
        );
    }
}
