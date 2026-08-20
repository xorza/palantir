//! One sweep: the point under the cursor stays under it at every scale.

use crate::Ui;
use crate::layout::types::sizing::Sizing;
use crate::primitives::translate_scale::TranslateScale;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::Configure;
use crate::ui::harness::UiHarness;
use crate::widgets::frame::Frame;
use crate::widgets::panel::Panel;
use crate::widgets::scroll::Scroll;
use crate::widgets::scroll::state::ScrollState;
use crate::widgets::scroll::tests::support::SURFACE;
use glam::Vec2;

#[test]
fn pointer_zoom_pivot_is_scale_invariant() {
    let id = WidgetId::from_hash("scaled-scroll");
    let logical_pointer = Vec2::new(50.0, 70.0);

    for scale in [0.5, 1.0, 2.0] {
        let mut h = UiHarness::new(SURFACE);
        let build = |ui: &mut Ui| {
            Panel::zstack()
                .id(WidgetId::from_hash("scaled-scroll-parent"))
                .transform(TranslateScale::from_scale(scale))
                .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                .show(ui, |ui| {
                    Scroll::both()
                        .id(id)
                        .with_zoom()
                        .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                        .show(ui, |ui| {
                            Frame::new()
                                .id(WidgetId::from_hash("scaled-scroll-content"))
                                .size((Sizing::fixed(400.0), Sizing::fixed(400.0)))
                                .show(ui);
                        });
                });
        };
        h.frame(build);

        let response = h.ui.response_for(id);
        let layout = response.layout_rect.expect("scroll arranged");
        let pointer = response.transform.apply_point(layout.min + logical_pointer);
        h.pinch_at(pointer, 1.5);
        h.frame(build);

        let state = *h.ui.state_mut::<ScrollState>(id);
        assert_eq!(state.zoom, 1.5, "zoom at {scale}×");
        assert_eq!(
            state.offset,
            logical_pointer * 0.5,
            "pointer pivot at {scale}×",
        );
    }
}

mod bars {
    use crate::Ui;
    use crate::layout::scrollbars::bar_geometry;
    use crate::layout::types::sizing::Sizing;
    use crate::primitives::background::Background;
    use crate::primitives::color::Color;
    use crate::primitives::rect::Rect;
    use crate::primitives::size::Size;
    use crate::primitives::widget_id::WidgetId;
    use crate::scene::layer::Layer;
    use crate::scene::node::Configure;
    use crate::scene::shapes::paint::QuadShape;
    use crate::scene::shapes::record::ShapeRecord;
    use crate::scene::tree::node_id::NodeId;
    use crate::shape::rect::RectKind;
    use crate::ui::frame_report::FrameProcessing;
    use crate::ui::harness::UiHarness;
    use crate::widgets::frame::Frame;
    use crate::widgets::panel::Panel;
    use crate::widgets::scroll::Scroll;
    use crate::widgets::scroll::state::ScrollState;
    use crate::widgets::scroll::tests::support::{scroll_content, scroll_viewport};
    use crate::widgets::theme::scrollbar::ScrollbarTheme;
    use glam::UVec2;
    use glam::Vec2;
    use std::time::Duration;

    fn theme() -> ScrollbarTheme {
        ScrollbarTheme::default()
    }

    /// `bar_geometry(viewport, content, offset, min_thumb)` returns
    /// `None` when content fits the viewport or the viewport collapses to
    /// zero; otherwise `Some { thumb_size, thumb_offset }`.
    ///
    /// The track spans the whole viewport extent, so one length drives
    /// both the `viewport / content` ratio and the travel:
    /// `thumb_size = clamp(viewport² / content, min_thumb, viewport)` and
    /// `thumb_offset = clamp(offset / (content - viewport), 0, 1) *
    /// (viewport - thumb_size)`.
    #[test]
    fn bar_geometry_thumb_size_and_offset_cases() {
        #[derive(Debug)]
        struct Want {
            thumb_size: Option<f32>,
            thumb_offset: Option<f32>,
        }
        type Case = (&'static str, f32, f32, f32, Option<Want>);
        let cases: &[Case] = &[
            (
                // 200² / 800 = 50, above the 24 px floor and under the
                // 200 px viewport, so the raw ratio survives both clamps.
                "ratio_above_floor",
                200.0,
                800.0,
                0.0,
                Some(Want {
                    thumb_size: Some(50.0),
                    thumb_offset: Some(0.0),
                }),
            ),
            (
                // Half of the 600 px scrollable range → half of the
                // 200 - 50 = 150 px travel.
                "midpoint_offset_rides_linearly",
                200.0,
                800.0,
                300.0,
                Some(Want {
                    thumb_size: Some(50.0),
                    thumb_offset: Some(75.0),
                }),
            ),
            (
                "max_offset_sits_at_track_end",
                200.0,
                800.0,
                600.0,
                Some(Want {
                    thumb_size: Some(50.0),
                    thumb_offset: Some(200.0 - 50.0),
                }),
            ),
            (
                // 100² / 10000 = 1 px, floored up to the theme minimum.
                "clamped_up_to_min_thumb_px",
                100.0,
                10_000.0,
                0.0,
                Some(Want {
                    thumb_size: Some(24.0),
                    thumb_offset: None,
                }),
            ),
            (
                // A viewport shorter than `min_thumb`: the floor would
                // overshoot the track, so the viewport cap wins.
                "clamped_down_to_viewport_when_min_exceeds_it",
                10.0,
                200.0,
                0.0,
                Some(Want {
                    thumb_size: Some(10.0),
                    thumb_offset: None,
                }),
            ),
            ("none_when_content_equals_viewport", 200.0, 200.0, 0.0, None),
            (
                "none_when_content_smaller_than_viewport",
                200.0,
                100.0,
                0.0,
                None,
            ),
            ("none_when_viewport_zero", 0.0, 800.0, 0.0, None),
        ];
        for (label, viewport, content, offset, want) in cases {
            let got = bar_geometry(*viewport, *content, *offset, theme().min_thumb_px);
            match (want, got) {
                (None, None) => {}
                (Some(want), Some(g)) => {
                    if let Some(s) = want.thumb_size {
                        assert!((g.thumb_size - s).abs() < 1e-3, "case: {label} thumb_size");
                    }
                    if let Some(o) = want.thumb_offset {
                        assert!(
                            (g.thumb_offset - o).abs() < 1e-3,
                            "case: {label} thumb_offset"
                        );
                    }
                }
                (want, got) => panic!(
                    "case: {label} mismatch: want={:?}, got={:?}",
                    want.is_some(),
                    got.is_some()
                ),
            }
        }
    }

    /// Build a scroll over two frames so the second frame's record
    /// settles `ScrollState` before the bar-emit check.
    fn record_two_frames<F: Fn(&mut Ui) + Copy>(surface: UVec2, build: F) -> (UiHarness, NodeId) {
        let mut h = UiHarness::new(surface);
        h.frame(build);
        h.frame(build);
        let scroll_id = WidgetId::from_hash("scroll");
        let idx =
            h.ui.tree(Layer::Main)
                .records
                .widget_id()
                .iter()
                .position(|w| *w == scroll_id)
                .expect("scroll widget recorded");
        (h, NodeId(idx as u32))
    }

    fn count_positioned(ui: &Ui, node: NodeId) -> usize {
        ui.tree(Layer::Main)
            .shapes_of(node)
            .filter(|s| {
                matches!(
                    s,
                    ShapeRecord::Quad(QuadShape::Rect {
                        kind: RectKind::Rounded,
                        local_rect: Some(_),
                        ..
                    })
                )
            })
            .count()
    }

    /// Thumb rects (in *outer-local* coords) for `scroll_key`. Thumbs
    /// are real `Sense::DRAG` leaf nodes under an overlay Canvas.
    /// Returns 0–2 rects (V and/or H) in vertical-then-horizontal order.
    fn thumb_rects(ui: &Ui, scroll_key: &str) -> Vec<Rect> {
        let tree = ui.tree(Layer::Main);
        let layout = ui.layout(Layer::Main);
        let outer_id = WidgetId::from_hash(scroll_key);
        let scroll_id = outer_id.with("viewport");
        let widget_ids = tree.records.widget_id();
        let outer_idx = widget_ids
            .iter()
            .position(|w| *w == outer_id)
            .expect("scroll outer recorded");
        let outer_origin = layout.rect[outer_idx].min;
        let mut out = Vec::new();
        for tag in ["vthumb", "hthumb"] {
            let id = scroll_id.with(tag);
            if let Some(idx) = widget_ids.iter().position(|w| *w == id) {
                let r = layout.rect[idx];
                // Both thumbs are recorded every frame — `layout::scrollbars`
                // collapses the ones with nothing to show to zero extent
                // rather than dropping them, so their ids and state rows
                // survive an overflow toggle. A collapsed thumb is not a
                // thumb, so it must not reach an assertion about placement.
                if r.size.w <= 0.0 || r.size.h <= 0.0 {
                    continue;
                }
                out.push(Rect {
                    min: r.min - outer_origin,
                    size: r.size,
                });
            }
        }
        out
    }

    #[test]
    fn hidden_scroll_skips_bar_ids_and_cold_relayout_but_keeps_pan_and_zoom() {
        let surface = UVec2::new(400, 400);
        let outer_id = WidgetId::from_hash("hidden-scroll");
        let scroll_id = outer_id.with("viewport");
        let build = |ui: &mut Ui| {
            Scroll::both()
                .id(outer_id)
                .hide_bars()
                .with_zoom()
                .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                .show(ui, |ui| {
                    Frame::new()
                        .id(WidgetId::from_hash("hidden-content"))
                        .size((Sizing::fixed(400.0), Sizing::fixed(400.0)))
                        .show(ui);
                });
        };

        let mut h = UiHarness::new(surface);
        let mut records = 0;
        let report = h.frame(|ui| {
            records += 1;
            build(ui);
        });
        assert_eq!(report.processing, FrameProcessing::SingleLayout);
        assert_eq!(
            records, 1,
            "hidden cold mount must not settle bar visibility"
        );

        let tree = h.ui.tree(Layer::Main);
        for tag in ["bars", "vtrack", "htrack", "vthumb", "hthumb"] {
            assert!(
                !tree
                    .records
                    .widget_id()
                    .iter()
                    .any(|widget_id| *widget_id == scroll_id.with(tag)),
                "hidden scroll recorded bar id {tag}",
            );
        }

        h.scroll_pixels_at(Vec2::new(50.0, 50.0), Vec2::new(40.0, 60.0));
        h.pinch(1.5);
        h.frame(build);
        let state = *h.ui.state_mut::<ScrollState>(outer_id);
        assert_eq!(scroll_viewport(&h.ui, outer_id), Size::new(200.0, 200.0));
        assert_eq!(state.zoom, 1.5);
        assert_eq!(
            state.offset,
            Vec2::new(65.0, 85.0),
            "pivot adds (25, 25), then wheel pan adds (40, 60)",
        );
    }

    #[test]
    fn vertical_overflow_emits_thumb_shape_after_settle() {
        let (ui, _node) = record_two_frames(UVec2::new(400, 600), |ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |ui| {
                    Scroll::vertical()
                        .id(WidgetId::from_hash("scroll"))
                        .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                        .show(ui, |ui| {
                            Frame::new()
                                .id(WidgetId::from_hash("tall"))
                                .size((Sizing::fixed(180.0), Sizing::fixed(800.0)))
                                .show(ui);
                        });
                });
        });
        assert!(
            !thumb_rects(&ui.ui, "scroll").is_empty(),
            "vertical overflow should emit at least one bar thumb"
        );
    }

    /// Content that stops overflowing must retire its bar, even though
    /// the bar overlay's own subtree hash and slot are unchanged — the
    /// showcase symptom was a scrollbar surviving a page switch.
    ///
    /// The bars' placement reads a *sibling's* measured `scroll_content`,
    /// so it is not the pure function of its own slot that arrange replay
    /// assumes; `LayoutEngine::arrange` exempts `Scrollbars` for exactly
    /// this. Asserting the raw rects (not `thumb_rects`, which filters
    /// collapsed bars) is what makes a stale bar visible to the test.
    #[test]
    fn content_that_stops_overflowing_retires_its_bar() {
        let build = |tall: bool| {
            move |ui: &mut Ui| {
                Panel::vstack()
                    .id(WidgetId::from_hash("root"))
                    .size((Sizing::fixed(400.0), Sizing::fixed(300.0)))
                    .show(ui, |ui| {
                        Scroll::vertical()
                            .id(WidgetId::from_hash("scroll"))
                            .size((Sizing::FILL, Sizing::FILL))
                            .overlay_bars()
                            .show(ui, |ui| {
                                Frame::new()
                                    .id(WidgetId::from_hash("body"))
                                    .size((
                                        Sizing::FILL,
                                        Sizing::fixed(if tall { 900.0 } else { 50.0 }),
                                    ))
                                    .show(ui);
                            });
                    });
            }
        };
        let surface = UVec2::new(400, 300);
        let mut h = UiHarness::new(surface);
        h.frame(build(true));
        h.frame(build(true));
        assert_eq!(
            thumb_rects(&h.ui, "scroll").len(),
            1,
            "900px of content in a 300px viewport must show a thumb",
        );

        h.frame(build(false));
        for (tag, rect) in raw_bar_rects(&h.ui, "scroll") {
            assert_eq!(
                rect.size,
                Size::ZERO,
                "{tag} must collapse once the content fits, got {rect:?}",
            );
        }

        // ...and come back, so the collapse isn't a one-way latch.
        h.frame(build(true));
        assert_eq!(
            thumb_rects(&h.ui, "scroll").len(),
            1,
            "the thumb must return when the content overflows again",
        );
    }

    /// Every bar node's arranged rect, collapsed ones included.
    fn raw_bar_rects(ui: &Ui, scroll_key: &str) -> Vec<(&'static str, Rect)> {
        let tree = ui.tree(Layer::Main);
        let layout = ui.layout(Layer::Main);
        let scroll_id = WidgetId::from_hash(scroll_key).with("viewport");
        let widget_ids = tree.records.widget_id();
        let mut out = Vec::new();
        for tag in ["vtrack", "vthumb", "htrack", "hthumb"] {
            let id = scroll_id.with(tag);
            if let Some(idx) = widget_ids.iter().position(|w| *w == id) {
                out.push((tag, layout.rect[idx]));
            }
        }
        out
    }

    /// A travelling thumb must not change *length* on screen. Physical
    /// snapping rounds a rect's min and max independently, so a thumb on
    /// fractional coordinates grows and shrinks by a pixel as it moves —
    /// the shimmer reported against the showcase. `axis_rects` pins the
    /// thumb to whole logical pixels to stop it; this asserts the
    /// *snapped* extent, since the logical one was already constant and
    /// so never caught the bug.
    #[test]
    fn a_travelling_thumb_keeps_its_snapped_length() {
        let build = |ui: &mut Ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .size((Sizing::fixed(400.0), Sizing::fixed(300.0)))
                .show(ui, |ui| {
                    Scroll::vertical()
                        .id(WidgetId::from_hash("scroll"))
                        .size((Sizing::FILL, Sizing::FILL))
                        .overlay_bars()
                        .gap(12.0)
                        .show(ui, |ui| {
                            for i in 0..8 {
                                Frame::new()
                                    .id(WidgetId::from_hash(format!("row{i}")))
                                    .size((Sizing::FILL, Sizing::fixed(90.0)))
                                    .show(ui);
                            }
                        });
                });
        };
        let surface = UVec2::new(400, 300);
        let mut h = UiHarness::new(surface);
        h.frame(build);
        h.frame(build);

        // What the compositor actually rasterizes, per `Rect::scaled_by`.
        let snapped = |r: Rect, scale: f32| (r.max().y * scale).round() - (r.min.y * scale).round();
        let first = thumb_rects(&h.ui, "scroll")[0];
        let expected: Vec<f32> = [1.0, 2.0, 3.0].iter().map(|s| snapped(first, *s)).collect();

        let mut travelled = Vec::new();
        for _ in 0..8 {
            h.scroll_pixels_at(Vec2::new(100.0, 100.0), Vec2::new(0.0, 37.0));
            h.frame(build);
            let r = thumb_rects(&h.ui, "scroll")[0];
            travelled.push(r.min.y);
            for (i, scale) in [1.0f32, 2.0, 3.0].iter().enumerate() {
                assert_eq!(
                    snapped(r, *scale),
                    expected[i],
                    "thumb length changed at DPR {scale} once it moved to y={}",
                    r.min.y,
                );
            }
        }
        assert!(
            travelled.windows(2).any(|w| w[0] != w[1]),
            "the thumb must actually travel for this to mean anything: {travelled:?}",
        );
    }

    /// Thumb *extent* is `viewport / content * track` — no offset term.
    /// Scrolling moves the thumb; it must never resize it.
    #[test]
    fn scrolling_moves_the_thumb_without_resizing_it() {
        let build = |ui: &mut Ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |ui| {
                    Scroll::vertical()
                        .id(WidgetId::from_hash("scroll"))
                        .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                        .show(ui, |ui| {
                            Frame::new()
                                .id(WidgetId::from_hash("tall"))
                                .size((Sizing::fixed(180.0), Sizing::fixed(800.0)))
                                .show(ui);
                        });
                });
        };
        let surface = UVec2::new(400, 600);
        let mut h = UiHarness::new(surface);
        h.frame(build);
        h.frame(build);
        let before = thumb_rects(&h.ui, "scroll");
        assert_eq!(before.len(), 1, "one vertical thumb");

        let mut seen = Vec::new();
        for _ in 0..4 {
            // Wheel input routes to whatever the pointer is over.
            h.scroll_pixels_at(Vec2::new(100.0, 100.0), Vec2::new(0.0, 50.0));
            h.frame(build);
            let now = thumb_rects(&h.ui, "scroll");
            assert_eq!(now.len(), 1, "thumb must not vanish mid-scroll");
            seen.push((now[0].min.y, now[0].size.h));
        }
        for (offset, height) in &seen {
            assert!(
                (height - before[0].size.h).abs() < 1e-3,
                "thumb resized while scrolling: {} -> {height} (offsets so far {seen:?})",
                before[0].size.h,
            );
            let _ = offset;
        }
        assert!(
            seen.windows(2).any(|w| w[0].0 != w[1].0),
            "the thumb should actually travel: {seen:?}",
        );
    }

    /// The reason `layout::scrollbars` exists. A scroll that has never
    /// recorded has no arranged viewport and no measured content — both
    /// terms of the thumb's size ratio — so it used to call
    /// `Ui::request_relayout` and re-record the entire frame to get them.
    /// The driver resolves them after measure instead, so the thumb is
    /// placed on the first painted frame *and* the frame stays one pass.
    #[test]
    fn cold_mount_places_the_thumb_in_one_record_pass() {
        let build = |ui: &mut Ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |ui| {
                    Scroll::vertical()
                        .id(WidgetId::from_hash("scroll"))
                        .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                        .show(ui, |ui| {
                            Frame::new()
                                .id(WidgetId::from_hash("tall"))
                                .size((Sizing::fixed(180.0), Sizing::fixed(800.0)))
                                .show(ui);
                        });
                });
        };
        let mut h = UiHarness::new(UVec2::new(400, 600));
        let mut passes = 0;
        let report = h.at(Duration::from_millis(16)).frame(|ui| {
            passes += 1;
            build(ui);
        });
        assert_eq!(passes, 1, "a cold-mounted scroll must not re-record");
        assert_eq!(report.processing, FrameProcessing::SingleLayout);

        // The vertical bar's gutter comes out of the *cross* axis (width),
        // so its own main extent is the full 200 — only a horizontal bar
        // would shorten it, and this scroll has none. Thumb is
        // `viewport/content * track` with track == viewport: 200/800*200.
        let theme = theme();
        let track: f32 = 200.0;
        let expected = (track / 800.0 * track).max(theme.min_thumb_px);
        assert_eq!(expected, 50.0, "arithmetic guard on the expectation");
        let thumbs = thumb_rects(&h.ui, "scroll");
        assert_eq!(thumbs.len(), 1, "one vertical thumb, no collapsed peers");
        assert!(
            (thumbs[0].size.h - expected).abs() < 1e-3,
            "first-frame thumb must already be sized from the measured \
             content: expected {expected}, got {}",
            thumbs[0].size.h,
        );
        assert_eq!(thumbs[0].size.w, theme.width);
    }

    #[test]
    fn no_bar_when_content_fits_viewport() {
        let (ui, node) = record_two_frames(UVec2::new(400, 400), |ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |ui| {
                    Scroll::vertical()
                        .id(WidgetId::from_hash("scroll"))
                        .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                        .show(ui, |ui| {
                            Frame::new()
                                .id(WidgetId::from_hash("short"))
                                .size((Sizing::fixed(180.0), Sizing::fixed(50.0)))
                                .show(ui);
                        });
                });
        });
        assert_eq!(
            count_positioned(&ui.ui, node),
            0,
            "non-overflowing content should produce no bar shapes"
        );
    }

    /// Repro for "PopClip without matching PushClip" panic — drive
    /// the full encode + compose pipeline twice (cold + warm caches)
    /// with a Scroll that emits bar shapes.
    #[test]
    fn scroll_with_bars_composes_through_warm_cache() {
        let surface = UVec2::new(400, 300);
        let mut h = UiHarness::new(surface);
        let build = |ui: &mut Ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |ui| {
                    Scroll::vertical()
                        .id(WidgetId::from_hash("scroll"))
                        .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                        .show(ui, |ui| {
                            for i in 0..30u32 {
                                Frame::new()
                                    .id(WidgetId::from_hash(("row", i)))
                                    .size((Sizing::fixed(180.0), Sizing::fixed(28.0)))
                                    .show(ui);
                            }
                        });
                });
        };
        h.frame(build);
        h.frame(build);
    }

    /// Showcase-style nested scroll cards. Pin that the deeper
    /// clip-stack walk + warm caches still leave the paint stream balanced.
    #[test]
    fn nested_clipped_scrolls_compose_through_warm_cache() {
        let surface = UVec2::new(800, 600);
        let mut h = UiHarness::new(surface);
        let build = |ui: &mut Ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("root"))
                .gap(12.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    for tag in ["v", "h", "xy"] {
                        Panel::vstack()
                            .id(WidgetId::from_hash(("card", tag)))
                            .padding(8.0)
                            .size((Sizing::FILL, Sizing::FILL))
                            .background(Background {
                                fill: Color::rgb(0.16, 0.20, 0.28).into(),
                                ..Default::default()
                            })
                            .clip_rect()
                            .show(ui, |ui| {
                                let s = match tag {
                                    "v" => {
                                        Scroll::vertical().id(WidgetId::from_hash(("scroll", tag)))
                                    }
                                    "h" => Scroll::horizontal()
                                        .id(WidgetId::from_hash(("scroll", tag))),
                                    _ => Scroll::both().id(WidgetId::from_hash(("scroll", tag))),
                                };
                                s.size((Sizing::FILL, Sizing::FILL)).show(ui, |ui| {
                                    for i in 0..40u32 {
                                        Frame::new()
                                            .id(WidgetId::from_hash((tag, "item", i)))
                                            .size((Sizing::fixed(120.0), Sizing::fixed(28.0)))
                                            .show(ui);
                                    }
                                });
                            });
                    }
                });
        };
        h.frame(build);
        h.frame(build);
        h.frame(build);
    }

    /// Reservation: when content overflows on the V axis, the inner
    /// shrinks by exactly `theme.width + theme.gap` on the right.
    #[test]
    fn vertical_overflow_reserves_bar_width_on_inner() {
        let surface = UVec2::new(400, 600);
        let mut h = UiHarness::new(surface);
        let build = |ui: &mut Ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |ui| {
                    Scroll::vertical()
                        .id(WidgetId::from_hash("scroll"))
                        .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                        .show(ui, |ui| {
                            Frame::new()
                                .id(WidgetId::from_hash("tall"))
                                .size((Sizing::fixed(180.0), Sizing::fixed(800.0)))
                                .show(ui);
                        });
                });
        };
        h.frame(build);
        h.frame(build);
        assert_eq!(
            scroll_viewport(&h.ui, WidgetId::from_hash("scroll")),
            Size::new(188.0, 200.0),
            "V overflow reserves theme.width + theme.gap = 12px on the right; H axis untouched"
        );
    }

    /// User-set padding is preserved — bar reservation adds to it.
    #[test]
    fn user_padding_is_preserved_when_bar_reserves() {
        let surface = UVec2::new(400, 600);
        let mut h = UiHarness::new(surface);
        let build = |ui: &mut Ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |ui| {
                    Scroll::vertical()
                        .id(WidgetId::from_hash("scroll"))
                        .padding(16.0)
                        .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                        .show(ui, |ui| {
                            Frame::new()
                                .id(WidgetId::from_hash("tall"))
                                .size((Sizing::fixed(100.0), Sizing::fixed(800.0)))
                                .show(ui);
                        });
                });
        };
        h.frame(build);
        h.frame(build);
        assert_eq!(
            scroll_viewport(&h.ui, WidgetId::from_hash("scroll")),
            Size::new(156.0, 168.0)
        );
    }

    /// Pin bar positioning: V bar's overlay rect sits flush with
    /// `outer.w - theme.width` (the reserved padding strip), NOT
    /// inside any user-set padding.
    #[test]
    fn vertical_bar_overlay_rect_lands_in_right_padding_strip() {
        let (ui, node) = record_two_frames(UVec2::new(400, 600), |ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |ui| {
                    Scroll::vertical()
                        .id(WidgetId::from_hash("scroll"))
                        .padding(16.0)
                        .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                        .show(ui, |ui| {
                            Frame::new()
                                .id(WidgetId::from_hash("tall"))
                                .size((Sizing::fixed(100.0), Sizing::fixed(800.0)))
                                .show(ui);
                        });
                });
        });
        let _ = node;
        let theme = theme();
        let expected_x = 200.0 - theme.width;
        let overlays = thumb_rects(&ui.ui, "scroll");
        assert!(!overlays.is_empty(), "expected at least one thumb");
        for r in &overlays {
            assert_eq!(
                r.min.x, expected_x,
                "V bar must sit at outer.w - theme.width (= reserved strip), \
                 not inside user padding"
            );
            assert_eq!(r.size.w, theme.width, "V bar width = theme.width");
        }
    }

    /// Reservation is **constant** across the overflow toggle — the
    /// viewport stays at `outer - bar_w` whether or not content
    /// overflows. Keeping the viewport size stable prevents Hug
    /// ancestors (e.g. a `Popup` body) from shifting by `bar_w` when
    /// overflow first appears. Bar visibility (thumb + track drawn or
    /// not) still toggles with overflow; only the gutter stays.
    #[test]
    fn bar_reservation_stays_constant_across_overflow_toggle() {
        let surface = UVec2::new(400, 600);
        let scroll_id = WidgetId::from_hash("scroll");
        let read_viewport = |ui: &Ui| scroll_viewport(ui, scroll_id);

        let build = |ui: &mut Ui, content_h: f32| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |ui| {
                    Scroll::vertical()
                        .id(WidgetId::from_hash("scroll"))
                        .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                        .show(ui, |ui| {
                            Frame::new()
                                .id(WidgetId::from_hash("body"))
                                .size((Sizing::fixed(180.0), Sizing::fixed(content_h)))
                                .show(ui);
                        });
                });
        };

        let mut h = UiHarness::new(surface);
        h.frame(|ui| build(ui, 800.0));
        h.frame(|ui| build(ui, 800.0));
        assert_eq!(
            read_viewport(&mut h.ui),
            Size::new(188.0, 200.0),
            "viewport = 200 - (width + gap) when content overflows",
        );

        h.frame(|ui| build(ui, 50.0));
        h.frame(|ui| build(ui, 50.0));
        assert_eq!(
            read_viewport(&mut h.ui),
            Size::new(188.0, 200.0),
            "viewport stays the same when content fits — gutter is constant",
        );
    }

    /// Zooming a `Scroll::both` shrinks the thumb proportionally to
    /// the content growth.
    #[test]
    fn zoomed_content_shrinks_thumb_proportionally() {
        let surface = UVec2::new(400, 400);
        let mut h = UiHarness::new(surface);
        let build = |ui: &mut Ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |ui| {
                    Scroll::both()
                        .id(WidgetId::from_hash("scroll"))
                        .with_zoom()
                        .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                        .show(ui, |ui| {
                            Frame::new()
                                .id(WidgetId::from_hash("big"))
                                .size((Sizing::fixed(400.0), Sizing::fixed(400.0)))
                                .show(ui);
                        });
                });
        };
        h.frame(build);
        h.frame(build);
        let scroll_id = WidgetId::from_hash("scroll");
        let z1_thumbs = thumb_rects(&h.ui, "scroll");
        assert_eq!(z1_thumbs.len(), 2, "z=1: V + H thumbs");
        let v1 = z1_thumbs
            .iter()
            .find(|r| r.size.h > r.size.w)
            .unwrap()
            .size
            .h;

        h.ui.state_mut::<ScrollState>(scroll_id).zoom = 2.0;
        h.frame(build);
        h.frame(build);
        let z2_thumbs = thumb_rects(&h.ui, "scroll");
        assert_eq!(z2_thumbs.len(), 2, "z=2: V + H thumbs");
        let v2 = z2_thumbs
            .iter()
            .find(|r| r.size.h > r.size.w)
            .unwrap()
            .size
            .h;
        assert!(v2 < v1, "thumb should shrink under zoom (v1={v1}, v2={v2})");
        let ratio = v2 / v1;
        assert!(
            (0.45..=0.55).contains(&ratio),
            "thumb shrink ratio off; v1={v1} v2={v2} ratio={ratio}"
        );
    }

    #[test]
    fn both_axes_overflow_emits_two_thumbs() {
        let (ui, _node) = record_two_frames(UVec2::new(400, 400), |ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |ui| {
                    Scroll::both()
                        .id(WidgetId::from_hash("scroll"))
                        .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                        .show(ui, |ui| {
                            Frame::new()
                                .id(WidgetId::from_hash("big"))
                                .size((Sizing::fixed(800.0), Sizing::fixed(800.0)))
                                .show(ui);
                        });
                });
        });
        assert_eq!(
            thumb_rects(&ui.ui, "scroll").len(),
            2,
            "ScrollXY with overflow on both axes should emit two thumbs"
        );
    }

    /// `ScrollXY` with both axes overflowing must NOT have its V and H
    /// bars overlap at the bottom-right corner.
    #[test]
    fn both_axes_bars_dont_overlap_at_corner() {
        let (ui, _node) = record_two_frames(UVec2::new(400, 400), |ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |ui| {
                    Scroll::both()
                        .id(WidgetId::from_hash("scroll"))
                        .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                        .show(ui, |ui| {
                            Frame::new()
                                .id(WidgetId::from_hash("big"))
                                .size((Sizing::fixed(800.0), Sizing::fixed(800.0)))
                                .show(ui);
                        });
                });
        });
        let theme = theme();
        let inner = 200.0 - theme.width - theme.gap;
        let outer_far = 200.0 - theme.width;
        let overlays = thumb_rects(&ui.ui, "scroll");
        assert_eq!(overlays.len(), 2, "expected V + H thumbs");
        let v = overlays
            .iter()
            .find(|r| r.min.x == outer_far)
            .expect("V bar at right edge");
        let h = overlays
            .iter()
            .find(|r| r.min.y == outer_far)
            .expect("H bar at bottom edge");
        assert!(
            v.max().y <= inner,
            "V bar must not extend into the H bar's reserved strip; \
             v.max.y={}, inner={inner}",
            v.max().y,
        );
        assert!(
            h.max().x <= inner,
            "H bar must not extend into the V bar's reserved strip; \
             h.max.x={}, inner={inner}",
            h.max().x,
        );
    }

    /// Cold-mount overflow must paint with the gutter reservation
    /// already in place on frame 1.
    #[test]
    fn cold_mount_overflow_paints_with_gutter_on_first_frame() {
        let surface = UVec2::new(400, 600);
        let mut h = UiHarness::new(surface);
        let theme = theme();
        let scroll_id = WidgetId::from_hash("scroll");
        let scene = |ui: &mut Ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |ui| {
                    Scroll::vertical()
                        .id(WidgetId::from_hash("scroll"))
                        .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                        .show(ui, |ui| {
                            Frame::new()
                                .id(WidgetId::from_hash("tall"))
                                .size((Sizing::fixed(180.0), Sizing::fixed(800.0)))
                                .show(ui);
                        });
                });
        };
        h.frame(scene);
        let expected = Size::new(200.0 - theme.width - theme.gap, 200.0);
        assert_eq!(
            scroll_viewport(&h.ui, scroll_id),
            expected,
            "cold-mount overflowing scroll: gutter reservation must be \
             active on the first painted frame; viewport should already \
             be deflated by `theme.width + theme.gap` on the cross axis",
        );
        assert!(
            scroll_content(&h.ui, scroll_id).h > scroll_viewport(&h.ui, scroll_id).h,
            "overflow flag must reflect post-relayout measurement (Y \
             overflows, X doesn't)",
        );
    }

    /// Cold-mount bar geometry must match steady-state frame-2 bar
    /// geometry.
    #[test]
    fn cold_mount_bar_geometry_matches_frame_two() {
        use crate::primitives::rect::Rect;
        let surface = UVec2::new(400, 600);
        let mut h = UiHarness::new(surface);
        let scene = |ui: &mut Ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |ui| {
                    Scroll::both()
                        .id(WidgetId::from_hash("scroll"))
                        .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                        .show(ui, |ui| {
                            Frame::new()
                                .id(WidgetId::from_hash("big"))
                                .size((Sizing::fixed(800.0), Sizing::fixed(800.0)))
                                .show(ui);
                        });
                });
        };
        let bar_rects = |ui: &Ui| -> Vec<Rect> {
            let mut rects = thumb_rects(ui, "scroll");
            rects.sort_by(|a, b| {
                a.min
                    .x
                    .total_cmp(&b.min.x)
                    .then(a.min.y.total_cmp(&b.min.y))
            });
            rects
        };

        h.frame(scene);
        let f1 = bar_rects(&h.ui);
        assert_eq!(f1.len(), 2, "cold-mount must emit both V + H thumbs");

        h.frame(scene);
        let f2 = bar_rects(&h.ui);

        assert_eq!(
            f1, f2,
            "bar shapes on cold-mount frame must match steady-state \
             frame 2 (regression: pass-B used pass-A's stale viewport \
             → bars shrank by theme.width + theme.gap on next frame)",
        );
    }

    /// `BarMode::Overlay`: no gutter is reserved. Viewport gets the
    /// full outer width regardless of content/overflow. The bar
    /// (when drawn) paints over the content's far-edge strip — same
    /// geometry as Reserved mode, but no space taken from the
    /// content area. Pinned with overflowing content so the bar
    /// would actually appear.
    #[test]
    fn overlay_mode_skips_gutter_reservation() {
        use crate::BarMode;
        let surface = UVec2::new(400, 600);
        let mut h = UiHarness::new(surface);
        let scroll_id = WidgetId::from_hash("scroll");
        let scene = |ui: &mut Ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |ui| {
                    Scroll::vertical()
                        .id(WidgetId::from_hash("scroll"))
                        .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                        .bar_mode(BarMode::Overlay)
                        .show(ui, |ui| {
                            Frame::new()
                                .id(WidgetId::from_hash("tall"))
                                .size((Sizing::fixed(180.0), Sizing::fixed(800.0)))
                                .show(ui);
                        });
                });
        };
        h.frame(scene);
        h.frame(scene);
        assert_eq!(
            scroll_viewport(&h.ui, scroll_id),
            Size::new(200.0, 200.0),
            "Overlay: viewport = full outer (no gutter reservation), \
             even when content overflows and the bar is drawn",
        );
        assert!(
            scroll_content(&h.ui, scroll_id).h > scroll_viewport(&h.ui, scroll_id).h,
            "content > viewport on Y — bar should be drawn"
        );
    }

    /// Cold-mount with content that fits in the viewport: the gutter
    /// is still reserved (constant), the bar thumb just isn't drawn.
    /// Overflow stays `false`.
    #[test]
    fn cold_mount_fits_reserves_gutter_but_paints_no_thumb() {
        let surface = UVec2::new(400, 600);
        let mut h = UiHarness::new(surface);
        let scroll_id = WidgetId::from_hash("scroll");
        let scene = |ui: &mut Ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |ui| {
                    Scroll::vertical()
                        .id(WidgetId::from_hash("scroll"))
                        .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                        .show(ui, |ui| {
                            Frame::new()
                                .id(WidgetId::from_hash("short"))
                                .size((Sizing::fixed(180.0), Sizing::fixed(50.0)))
                                .show(ui);
                        });
                });
        };
        h.frame(scene);
        assert_eq!(
            scroll_viewport(&h.ui, scroll_id),
            Size::new(188.0, 200.0),
            "gutter is constant — viewport = outer - (width + gap) even with no overflow",
        );
        assert_eq!(scroll_content(&h.ui, scroll_id), Size::new(180.0, 50.0));
        assert!(
            scroll_content(&h.ui, scroll_id).w <= scroll_viewport(&h.ui, scroll_id).w
                && scroll_content(&h.ui, scroll_id).h <= scroll_viewport(&h.ui, scroll_id).h
        );
    }
}
