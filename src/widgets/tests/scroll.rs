use crate::Ui;
use crate::forest::element::Configure;
use crate::forest::tree::Layer;
use crate::input::InputEvent;
use crate::layout::scroll::ScrollLayoutState as ScrollState;
use crate::layout::types::sizing::Sizing;
use crate::primitives::size::Size;
use crate::primitives::widget_id::WidgetId;
use crate::ui::test_support::new_ui;
use crate::widgets::frame::Frame;
use crate::widgets::panel::Panel;
use crate::widgets::scroll::Scroll;
use glam::{UVec2, Vec2};

const SURFACE: UVec2 = UVec2::new(400, 600);

fn build(ui: &mut Ui, viewport_h: f32, content_h: f32) {
    Panel::vstack().id_salt("root").show(ui, |ui| {
        Scroll::vertical()
            .id_salt("scroll")
            .size((Sizing::Fixed(200.0), Sizing::Fixed(viewport_h)))
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("content")
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(content_h)))
                    .show(ui);
            });
    });
}

fn read_state(ui: &mut Ui) -> ScrollState {
    *ui.scroll_state(WidgetId::from_hash("scroll").with("__viewport"))
}

#[test]
fn scroll_state_records_viewport_and_content_after_arrange() {
    let mut ui = new_ui();
    ui.run_at_acked(SURFACE, |ui| build(ui, 200.0, 800.0));
    let row = read_state(&mut ui);
    assert_eq!(row.viewport.h, 200.0);
    assert_eq!(row.content.h, 800.0);
    assert_eq!(row.offset, Vec2::ZERO, "no wheel input → offset stays at 0");
}

/// Wheel delta accumulates across frames into offset, clamped to
/// `[0, content - viewport]`. When content fits inside the viewport,
/// the offset stays at zero.
#[test]
fn wheel_delta_advances_offset_with_clamp() {
    let cases: &[(&str, f32, f32, &[f32], f32)] = &[
        ("single_push_accumulates", 200.0, 800.0, &[50.0], 50.0),
        (
            "second_push_accumulates_and_clamps_at_max",
            200.0,
            800.0,
            &[50.0, 9_999.0],
            600.0,
        ),
        ("non_overflowing_stays_zero", 300.0, 100.0, &[500.0], 0.0),
    ];
    for (label, viewport_h, content_h, pushes, expected) in cases {
        let mut ui = new_ui();
        ui.run_at_acked(SURFACE, |ui| build(ui, *viewport_h, *content_h));
        ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
        for wheel_y in *pushes {
            ui.on_input(InputEvent::ScrollPixels(Vec2::new(0.0, *wheel_y)));
            ui.run_at_acked(SURFACE, |ui| build(ui, *viewport_h, *content_h));
        }

        assert_eq!(read_state(&mut ui).offset.y, *expected, "case: {label}");
    }
}

#[test]
fn horizontal_scroll_pans_only_x() {
    let mut ui = new_ui();
    let build_h = |ui: &mut Ui| {
        Panel::vstack().id_salt("root").show(ui, |ui| {
            Scroll::horizontal()
                .id_salt("hscroll")
                .size((Sizing::Fixed(200.0), Sizing::Fixed(40.0)))
                .show(ui, |ui| {
                    Frame::new()
                        .id_salt("hcontent")
                        .size((Sizing::Fixed(800.0), Sizing::Fixed(40.0)))
                        .show(ui);
                });
        });
    };
    ui.run_at_acked(SURFACE, build_h);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 20.0)));
    ui.on_input(InputEvent::ScrollPixels(Vec2::new(75.0, 200.0)));

    ui.run_at_acked(SURFACE, build_h);
    let id = WidgetId::from_hash("hscroll").with("__viewport");
    let row = *ui.scroll_state(id);
    assert_eq!(row.offset, Vec2::new(75.0, 0.0));
}

#[test]
fn both_axis_scroll_pans_both_axes() {
    let mut ui = new_ui();
    let build_xy = |ui: &mut Ui| {
        Panel::vstack().id_salt("root").show(ui, |ui| {
            Scroll::both()
                .id_salt("xy")
                .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                .show(ui, |ui| {
                    Frame::new()
                        .id_salt("xy-content")
                        .size((Sizing::Fixed(800.0), Sizing::Fixed(800.0)))
                        .show(ui);
                });
        });
    };
    ui.run_at_acked(SURFACE, build_xy);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    ui.on_input(InputEvent::ScrollPixels(Vec2::new(40.0, 60.0)));

    ui.run_at_acked(SURFACE, build_xy);
    let id = WidgetId::from_hash("xy").with("__viewport");
    let row = *ui.scroll_state(id);
    assert_eq!(row.offset, Vec2::new(40.0, 60.0));
    assert_eq!(row.content, Size::new(800.0, 800.0));
    // Viewport reserves `theme.width + theme.gap = 12px` per panned
    // axis when content overflows; 200 - 12 = 188.
    assert_eq!(row.viewport, Size::new(188.0, 188.0));
}

/// `ScrollState.content` records the content extent the scroll
/// viewport sees. V-axis and H-axis behave like a Stack: sum along
/// the panned axis, max on the cross. XY behaves like a ZStack: max
/// per axis. An empty scroll records zero.
#[test]
fn scroll_records_content_extent() {
    enum Axis {
        V,
        H,
        XY,
        Empty,
    }
    let cases: &[(&str, Axis, &str, Size)] = &[
        (
            "v_axis_sum_main_max_cross",
            Axis::V,
            "scroll",
            Size::new(180.0, 92.0),
        ),
        (
            "h_axis_sum_main_max_cross",
            Axis::H,
            "scroll",
            Size::new(128.0, 40.0),
        ),
        (
            "xy_max_per_axis",
            Axis::XY,
            "scroll",
            Size::new(300.0, 250.0),
        ),
        ("empty_records_zero", Axis::Empty, "empty", Size::ZERO),
    ];
    for (label, axis, scroll_key, expected) in cases {
        let mut ui = new_ui();
        let surface = match axis {
            Axis::V | Axis::Empty => UVec2::new(400, 600),
            Axis::H => UVec2::new(800, 200),
            Axis::XY => UVec2::new(400, 400),
        };
        let scroll_node = ui.under_outer(surface, |ui| match axis {
            Axis::V => Scroll::vertical()
                .id_salt("scroll")
                .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                .gap(4.0)
                .show(ui, |ui| {
                    for i in 0..3u32 {
                        Frame::new()
                            .id_salt(("row", i))
                            .size((Sizing::Fixed(180.0), Sizing::Fixed(28.0)))
                            .show(ui);
                    }
                })
                .node(ui),
            Axis::H => Scroll::horizontal()
                .id_salt("scroll")
                .size((Sizing::Fixed(200.0), Sizing::Fixed(60.0)))
                .gap(8.0)
                .show(ui, |ui| {
                    for i in 0..2u32 {
                        Frame::new()
                            .id_salt(("col", i))
                            .size((Sizing::Fixed(60.0), Sizing::Fixed(40.0)))
                            .show(ui);
                    }
                })
                .node(ui),
            Axis::XY => Scroll::both()
                .id_salt("scroll")
                .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
                .show(ui, |ui| {
                    Frame::new()
                        .id_salt("wide")
                        .size((Sizing::Fixed(300.0), Sizing::Fixed(60.0)))
                        .show(ui);
                    Frame::new()
                        .id_salt("tall")
                        .size((Sizing::Fixed(80.0), Sizing::Fixed(250.0)))
                        .show(ui);
                })
                .node(ui),
            Axis::Empty => Scroll::vertical()
                .id_salt("empty")
                .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
                .show(ui, |_| {})
                .node(ui),
        });
        let scroll_id = WidgetId::from_hash(scroll_key).with("__viewport");
        let state = *ui.scroll_state(scroll_id);
        assert_eq!(state.content, *expected, "case: {label} content");
        let rect = ui.layout[Layer::Main].rect[scroll_node.index()];
        let want_view = match axis {
            Axis::V => (200.0, 200.0),
            Axis::H => (200.0, 60.0),
            Axis::XY | Axis::Empty => (100.0, 100.0),
        };
        assert_eq!(
            (rect.size.w, rect.size.h),
            want_view,
            "case: {label} viewport"
        );
    }
}

/// Two identical frames: first populates `ScrollState.content` from
/// the live measure; second is a measure-cache hit at an ancestor —
/// the Scroll's measure arm doesn't fire, so no write to `content`
/// happens this frame. The previous frame's `ScrollState.content`
/// stays valid because cache-hit ⟹ byte-identical measure output.
#[test]
fn scroll_state_content_survives_measure_cache_hit() {
    let surface = UVec2::new(400, 600);
    let build = |ui: &mut Ui| {
        Panel::vstack().id_salt("root").show(ui, |ui| {
            Scroll::vertical()
                .id_salt("scroll")
                .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                .gap(4.0)
                .show(ui, |ui| {
                    for i in 0..3u32 {
                        Frame::new()
                            .id_salt(("row", i))
                            .size((Sizing::Fixed(180.0), Sizing::Fixed(28.0)))
                            .show(ui);
                    }
                });
        });
    };

    let mut ui = new_ui();
    ui.run_at_acked(surface, build);
    let scroll_id = WidgetId::from_hash("scroll").with("__viewport");
    let after_first = *ui.scroll_state(scroll_id);
    assert_eq!(after_first.content.h, 92.0);

    ui.run_at_acked(surface, build);
    let after_second = *ui.scroll_state(scroll_id);
    assert_eq!(
        after_second.content, after_first.content,
        "ScrollState.content survives a measure-cache hit",
    );
    assert_eq!(after_second.viewport, after_first.viewport);
}

#[test]
fn pinch_zoom_keeps_point_under_cursor_fixed() {
    use crate::widgets::scroll::{ZoomConfig, ZoomPivot};

    const OUTER_PAD: f32 = 16.0;
    const TEXT_GAP: f32 = 24.0;

    struct Case {
        label: &'static str,
        content_size: f32,
        pans: &'static [(f32, f32)],
        pointer: (f32, f32),
        pinches: &'static [f32],
    }
    let cases: &[Case] = &[
        Case {
            label: "zoom_in_overflow_single",
            content_size: 800.0,
            pans: &[(40.0, 60.0)],
            pointer: (OUTER_PAD + 50.0, OUTER_PAD + TEXT_GAP + 70.0),
            pinches: &[1.5],
        },
        Case {
            label: "zoom_out_overflow_single",
            content_size: 800.0,
            pans: &[(120.0, 90.0)],
            pointer: (OUTER_PAD + 30.0, OUTER_PAD + TEXT_GAP + 40.0),
            pinches: &[0.7],
        },
        Case {
            label: "zoom_out_underflow_single",
            content_size: 100.0,
            pans: &[],
            pointer: (OUTER_PAD + 50.0, OUTER_PAD + TEXT_GAP + 70.0),
            pinches: &[0.5],
        },
        Case {
            label: "zoom_in_continuous_many_small_steps",
            content_size: 800.0,
            pans: &[(40.0, 60.0)],
            pointer: (OUTER_PAD + 80.0, OUTER_PAD + TEXT_GAP + 110.0),
            pinches: &[1.02; 30],
        },
        Case {
            label: "zoom_out_continuous_through_underflow",
            content_size: 300.0,
            pans: &[],
            pointer: (OUTER_PAD + 60.0, OUTER_PAD + TEXT_GAP + 90.0),
            pinches: &[0.97; 40],
        },
    ];

    for case in cases {
        let Case {
            label,
            content_size,
            pans,
            pointer,
            pinches,
        } = *case;
        let mut ui = new_ui();
        let build = |ui: &mut Ui| {
            Panel::vstack()
                .id_salt("root")
                .padding(OUTER_PAD)
                .show(ui, |ui| {
                    Frame::new()
                        .id_salt("topbar")
                        .size((Sizing::Fixed(200.0), Sizing::Fixed(TEXT_GAP)))
                        .show(ui);
                    Scroll::both()
                        .id_salt("xy")
                        .with_zoom_config(ZoomConfig {
                            pivot: ZoomPivot::Pointer,
                            ..ZoomConfig::default()
                        })
                        .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                        .show(ui, |ui| {
                            Frame::new()
                                .id_salt("content")
                                .size((Sizing::Fixed(content_size), Sizing::Fixed(content_size)))
                                .show(ui);
                        });
                });
        };

        ui.run_at_acked(SURFACE, build);

        ui.on_input(InputEvent::PointerMoved(Vec2::new(pointer.0, pointer.1)));
        for &(px, py) in pans {
            ui.on_input(InputEvent::ScrollPixels(Vec2::new(px, py)));
            ui.run_at_acked(SURFACE, build);
        }

        let id = WidgetId::from_hash("xy").with("__viewport");
        let before = *ui.scroll_state(id);
        let pivot_local = Vec2::new(pointer.0 - OUTER_PAD, pointer.1 - (OUTER_PAD + TEXT_GAP));
        let world_before = Vec2::new(
            (pivot_local.x + before.offset.x) / before.zoom,
            (pivot_local.y + before.offset.y) / before.zoom,
        );

        for &pinch in pinches {
            ui.on_input(InputEvent::Zoom(pinch));
            ui.run_at_acked(SURFACE, build);
        }

        let after = *ui.scroll_state(id);
        let world_after = Vec2::new(
            (pivot_local.x + after.offset.x) / after.zoom,
            (pivot_local.y + after.offset.y) / after.zoom,
        );

        let dx = (world_after.x - world_before.x).abs();
        let dy = (world_after.y - world_before.y).abs();
        assert!(
            dx < 1e-2 && dy < 1e-2,
            "case {label}: inner-local world point drifted \
             before=({:.3},{:.3}) after=({:.3},{:.3}) \
             (zoom {} → {}, offset {:?} → {:?})",
            world_before.x,
            world_before.y,
            world_after.x,
            world_after.y,
            before.zoom,
            after.zoom,
            before.offset,
            after.offset,
        );
        let inner_origin = Vec2::new(OUTER_PAD, OUTER_PAD + TEXT_GAP);
        let predicted_screen = Vec2::new(
            inner_origin.x + world_after.x * after.zoom - after.offset.x,
            inner_origin.y + world_after.y * after.zoom - after.offset.y,
        );
        let sx = (predicted_screen.x - pointer.0).abs();
        let sy = (predicted_screen.y - pointer.1).abs();
        assert!(
            sx < 1e-2 && sy < 1e-2,
            "case {label}: world point doesn't land on cursor in screen coords \
             predicted={:?} cursor=({},{}) (zoom {} → {}, offset {:?} → {:?})",
            predicted_screen,
            pointer.0,
            pointer.1,
            before.zoom,
            after.zoom,
            before.offset,
            after.offset,
        );
        assert!(
            (after.zoom - before.zoom).abs() > 1e-4,
            "case {label}: zoom didn't change ({} → {})",
            before.zoom,
            after.zoom,
        );
    }
}

/// Pivot-anchored zoom can leave `offset` outside the natural pan
/// range `[min(0, slack), max(0, slack)]`. A wheel-pan in that frame
/// must NOT yank `offset` back into `[0, slack]` (the visible "snap
/// to top" when the bar reappears). Rubber-band: pan toward the
/// natural range works, pan further out is blocked.
#[test]
fn pan_after_pivot_zoom_does_not_snap_out_of_range_offset() {
    use crate::widgets::scroll::{ZoomConfig, ZoomPivot};

    let mut ui = new_ui();
    let build = |ui: &mut Ui| {
        Panel::vstack().id_salt("root").show(ui, |ui| {
            Scroll::both()
                .id_salt("xy")
                .with_zoom_config(ZoomConfig {
                    pivot: ZoomPivot::Pointer,
                    ..ZoomConfig::default()
                })
                .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                .show(ui, |ui| {
                    Frame::new()
                        .id_salt("content")
                        .size((Sizing::Fixed(400.0), Sizing::Fixed(400.0)))
                        .show(ui);
                });
        });
    };
    ui.run_at_acked(SURFACE, build);

    let id = WidgetId::from_hash("xy").with("__viewport");
    {
        let row = ui.scroll_state(id);
        row.offset = Vec2::new(0.0, -50.0);
    }

    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    ui.on_input(InputEvent::ScrollPixels(Vec2::new(0.0, 5.0)));
    ui.run_at_acked(SURFACE, build);

    let after = *ui.scroll_state(id);
    assert!(
        (after.offset.y - (-45.0)).abs() < 1e-3,
        "wheel pan from out-of-range offset snapped: -50 + 5 should be -45, got {}",
        after.offset.y,
    );

    ui.on_input(InputEvent::ScrollPixels(Vec2::new(0.0, -5.0)));
    ui.run_at_acked(SURFACE, build);
    let after2 = *ui.scroll_state(id);
    assert!(
        (after2.offset.y - (-45.0)).abs() < 1e-3,
        "pan further out-of-range should be blocked at current ({}), got {}",
        -45.0,
        after2.offset.y,
    );
}

mod bars {
    use crate::Ui;
    use crate::forest::element::Configure;
    use crate::forest::shapes::record::ShapeRecord;
    use crate::forest::tree::{Layer, NodeId};
    use crate::layout::types::sizing::Sizing;
    use crate::primitives::background::Background;
    use crate::primitives::widget_id::WidgetId;
    use crate::ui::test_support::new_ui;
    use crate::widgets::frame::Frame;
    use crate::widgets::panel::Panel;
    use crate::widgets::scroll::{Scroll, bar_geometry};
    use crate::widgets::theme::ScrollbarTheme;
    use glam::UVec2;

    fn theme() -> ScrollbarTheme {
        ScrollbarTheme::default()
    }

    /// `bar_geometry(viewport, content, offset, track, theme)` returns
    /// `None` when content fits the viewport or the track collapses to
    /// zero; otherwise `Some { thumb_size, thumb_offset }`.
    #[test]
    fn bar_geometry_thumb_size_and_offset_cases() {
        struct Want {
            thumb_size: Option<f32>,
            thumb_offset: Option<f32>,
        }
        type Case = (&'static str, f32, f32, f32, f32, Option<Want>);
        let cases: &[Case] = &[
            (
                "ratio_above_floor",
                200.0,
                800.0,
                0.0,
                180.0,
                Some(Want {
                    thumb_size: Some(45.0),
                    thumb_offset: Some(0.0),
                }),
            ),
            (
                "midpoint_offset_rides_linearly",
                200.0,
                800.0,
                300.0,
                180.0,
                Some(Want {
                    thumb_size: Some(45.0),
                    thumb_offset: Some(67.5),
                }),
            ),
            (
                "max_offset_sits_at_track_end",
                200.0,
                800.0,
                600.0,
                180.0,
                Some(Want {
                    thumb_size: Some(45.0),
                    thumb_offset: Some(180.0 - 45.0),
                }),
            ),
            (
                "clamped_up_to_min_thumb_px",
                100.0,
                10_000.0,
                0.0,
                180.0,
                Some(Want {
                    thumb_size: Some(24.0),
                    thumb_offset: None,
                }),
            ),
            (
                "clamped_down_to_track_when_min_exceeds_track",
                100.0,
                200.0,
                0.0,
                10.0,
                Some(Want {
                    thumb_size: Some(10.0),
                    thumb_offset: None,
                }),
            ),
            (
                "none_when_content_equals_viewport",
                200.0,
                200.0,
                0.0,
                180.0,
                None,
            ),
            (
                "none_when_content_smaller_than_viewport",
                200.0,
                100.0,
                0.0,
                180.0,
                None,
            ),
            ("none_when_track_zero", 200.0, 800.0, 0.0, 0.0, None),
        ];
        for (label, viewport, content, offset, track, want) in cases {
            let got = bar_geometry(*viewport, *content, *offset, *track, &theme());
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
    fn record_two_frames<F: Fn(&mut Ui) + Copy>(surface: UVec2, build: F) -> (Ui, NodeId) {
        let mut ui = new_ui();
        ui.run_at_acked(surface, build);
        ui.run_at_acked(surface, build);
        let scroll_id = WidgetId::from_hash("scroll");
        let idx = ui
            .forest
            .tree(Layer::Main)
            .records
            .widget_id()
            .iter()
            .position(|w| *w == scroll_id)
            .expect("scroll widget recorded");
        (ui, NodeId(idx as u32))
    }

    fn count_positioned(ui: &Ui, node: NodeId) -> usize {
        ui.forest
            .tree(Layer::Main)
            .shapes_of(node)
            .filter(|s| {
                matches!(
                    s,
                    ShapeRecord::RoundedRect {
                        local_rect: Some(_),
                        ..
                    }
                )
            })
            .count()
    }

    /// Thumb rects (in *outer-local* coords) for `scroll_key`. Thumbs
    /// are real `Sense::DRAG` leaf nodes under an overlay Canvas.
    /// Returns 0–2 rects (V and/or H) in vertical-then-horizontal order.
    fn thumb_rects(ui: &Ui, scroll_key: &str) -> Vec<crate::primitives::rect::Rect> {
        let tree = ui.forest.tree(Layer::Main);
        let layout = &ui.layout[Layer::Main];
        let outer_id = WidgetId::from_hash(scroll_key);
        let scroll_id = outer_id.with("__viewport");
        let widget_ids = tree.records.widget_id();
        let outer_idx = widget_ids
            .iter()
            .position(|w| *w == outer_id)
            .expect("scroll outer recorded");
        let outer_origin = layout.rect[outer_idx].min;
        let mut out = Vec::new();
        for tag in ["__vthumb", "__hthumb"] {
            let id = scroll_id.with(tag);
            if let Some(idx) = widget_ids.iter().position(|w| *w == id) {
                let r = layout.rect[idx];
                out.push(crate::primitives::rect::Rect {
                    min: r.min - outer_origin,
                    size: r.size,
                });
            }
        }
        out
    }

    #[test]
    fn vertical_overflow_emits_thumb_shape_after_settle() {
        let (ui, _node) = record_two_frames(UVec2::new(400, 600), |ui| {
            Panel::vstack().id_salt("root").show(ui, |ui| {
                Scroll::vertical()
                    .id_salt("scroll")
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .id_salt("tall")
                            .size((Sizing::Fixed(180.0), Sizing::Fixed(800.0)))
                            .show(ui);
                    });
            });
        });
        assert!(
            !thumb_rects(&ui, "scroll").is_empty(),
            "vertical overflow should emit at least one bar thumb"
        );
    }

    #[test]
    fn no_bar_when_content_fits_viewport() {
        let (ui, node) = record_two_frames(UVec2::new(400, 400), |ui| {
            Panel::vstack().id_salt("root").show(ui, |ui| {
                Scroll::vertical()
                    .id_salt("scroll")
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .id_salt("short")
                            .size((Sizing::Fixed(180.0), Sizing::Fixed(50.0)))
                            .show(ui);
                    });
            });
        });
        assert_eq!(
            count_positioned(&ui, node),
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
        let mut ui = new_ui();
        let build = |ui: &mut Ui| {
            Panel::vstack().id_salt("root").show(ui, |ui| {
                Scroll::vertical()
                    .id_salt("scroll")
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                    .show(ui, |ui| {
                        for i in 0..30u32 {
                            Frame::new()
                                .id_salt(("row", i))
                                .size((Sizing::Fixed(180.0), Sizing::Fixed(28.0)))
                                .show(ui);
                        }
                    });
            });
        };
        ui.run_at_acked(surface, build);
        ui.run_at_acked(surface, build);
    }

    /// Showcase-style nested scroll cards. Pin that the deeper
    /// clip-stack walk + warm caches still leave the cmd stream balanced.
    #[test]
    fn nested_clipped_scrolls_compose_through_warm_cache() {
        let surface = UVec2::new(800, 600);
        let mut ui = new_ui();
        let build = |ui: &mut Ui| {
            Panel::hstack()
                .id_salt("root")
                .gap(12.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    for tag in ["v", "h", "xy"] {
                        Panel::vstack()
                            .id_salt(("card", tag))
                            .padding(8.0)
                            .size((Sizing::FILL, Sizing::FILL))
                            .background(Background {
                                fill: crate::primitives::color::Color::rgb(0.16, 0.20, 0.28).into(),
                                ..Default::default()
                            })
                            .clip_rect()
                            .show(ui, |ui| {
                                let s = match tag {
                                    "v" => Scroll::vertical().id_salt(("scroll", tag)),
                                    "h" => Scroll::horizontal().id_salt(("scroll", tag)),
                                    _ => Scroll::both().id_salt(("scroll", tag)),
                                };
                                s.size((Sizing::FILL, Sizing::FILL)).show(ui, |ui| {
                                    for i in 0..40u32 {
                                        Frame::new()
                                            .id_salt((tag, "item", i))
                                            .size((Sizing::Fixed(120.0), Sizing::Fixed(28.0)))
                                            .show(ui);
                                    }
                                });
                            });
                    }
                });
        };
        ui.run_at_acked(surface, build);
        ui.run_at_acked(surface, build);
        ui.run_at_acked(surface, build);
    }

    /// Reservation: when content overflows on the V axis, the inner
    /// shrinks by exactly `theme.width + theme.gap` on the right.
    #[test]
    fn vertical_overflow_reserves_bar_width_on_inner() {
        use crate::primitives::size::Size;
        let surface = UVec2::new(400, 600);
        let mut ui = new_ui();
        let build = |ui: &mut Ui| {
            Panel::vstack().id_salt("root").show(ui, |ui| {
                Scroll::vertical()
                    .id_salt("scroll")
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .id_salt("tall")
                            .size((Sizing::Fixed(180.0), Sizing::Fixed(800.0)))
                            .show(ui);
                    });
            });
        };
        ui.run_at_acked(surface, build);
        ui.run_at_acked(surface, build);
        let row = *ui.scroll_state(WidgetId::from_hash("scroll").with("__viewport"));
        assert_eq!(
            row.viewport,
            Size::new(188.0, 200.0),
            "V overflow reserves theme.width + theme.gap = 12px on the right; H axis untouched"
        );
    }

    /// User-set padding is preserved — bar reservation adds to it.
    #[test]
    fn user_padding_is_preserved_when_bar_reserves() {
        use crate::primitives::size::Size;
        let surface = UVec2::new(400, 600);
        let mut ui = new_ui();
        let build = |ui: &mut Ui| {
            Panel::vstack().id_salt("root").show(ui, |ui| {
                Scroll::vertical()
                    .id_salt("scroll")
                    .padding(16.0)
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .id_salt("tall")
                            .size((Sizing::Fixed(100.0), Sizing::Fixed(800.0)))
                            .show(ui);
                    });
            });
        };
        ui.run_at_acked(surface, build);
        ui.run_at_acked(surface, build);
        let row = *ui.scroll_state(WidgetId::from_hash("scroll").with("__viewport"));
        assert_eq!(row.viewport, Size::new(156.0, 168.0));
    }

    /// Pin bar positioning: V bar's overlay rect sits flush with
    /// `outer.w - theme.width` (the reserved padding strip), NOT
    /// inside any user-set padding.
    #[test]
    fn vertical_bar_overlay_rect_lands_in_right_padding_strip() {
        let (ui, node) = record_two_frames(UVec2::new(400, 600), |ui| {
            Panel::vstack().id_salt("root").show(ui, |ui| {
                Scroll::vertical()
                    .id_salt("scroll")
                    .padding(16.0)
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .id_salt("tall")
                            .size((Sizing::Fixed(100.0), Sizing::Fixed(800.0)))
                            .show(ui);
                    });
            });
        });
        let _ = node;
        let theme = theme();
        let expected_x = 200.0 - theme.width;
        let overlays = thumb_rects(&ui, "scroll");
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

    /// Reservation must collapse when overflow goes away.
    #[test]
    fn bar_reservation_collapses_when_overflow_disappears() {
        use crate::primitives::size::Size;
        let surface = UVec2::new(400, 600);
        let scroll_id = WidgetId::from_hash("scroll").with("__viewport");
        let read_viewport = |ui: &mut Ui| ui.scroll_state(scroll_id).viewport;

        let build = |ui: &mut Ui, content_h: f32| {
            Panel::vstack().id_salt("root").show(ui, |ui| {
                Scroll::vertical()
                    .id_salt("scroll")
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .id_salt("body")
                            .size((Sizing::Fixed(180.0), Sizing::Fixed(content_h)))
                            .show(ui);
                    });
            });
        };

        let mut ui = new_ui();
        ui.run_at_acked(surface, |ui| build(ui, 800.0));
        ui.run_at_acked(surface, |ui| build(ui, 800.0));
        assert_eq!(
            read_viewport(&mut ui),
            Size::new(188.0, 200.0),
            "frame 2: reservation active, viewport = 200 - (width + gap)"
        );

        ui.run_at_acked(surface, |ui| build(ui, 50.0));
        ui.run_at_acked(surface, |ui| build(ui, 50.0));
        assert_eq!(
            read_viewport(&mut ui),
            Size::new(200.0, 200.0),
            "after content shrinks, reservation collapses; viewport = full outer"
        );
    }

    /// Zooming a `Scroll::both` shrinks the thumb proportionally to
    /// the content growth.
    #[test]
    fn zoomed_content_shrinks_thumb_proportionally() {
        let surface = UVec2::new(400, 400);
        let mut ui = new_ui();
        let build = |ui: &mut Ui| {
            Panel::vstack().id_salt("root").show(ui, |ui| {
                Scroll::both()
                    .id_salt("scroll")
                    .with_zoom()
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .id_salt("big")
                            .size((Sizing::Fixed(400.0), Sizing::Fixed(400.0)))
                            .show(ui);
                    });
            });
        };
        ui.run_at_acked(surface, build);
        ui.run_at_acked(surface, build);
        let scroll_id = WidgetId::from_hash("scroll").with("__viewport");
        let z1_thumbs = thumb_rects(&ui, "scroll");
        assert_eq!(z1_thumbs.len(), 2, "z=1: V + H thumbs");
        let v1 = z1_thumbs
            .iter()
            .find(|r| r.size.h > r.size.w)
            .unwrap()
            .size
            .h;

        ui.scroll_state(scroll_id).zoom = 2.0;
        ui.run_at_acked(surface, build);
        ui.run_at_acked(surface, build);
        let z2_thumbs = thumb_rects(&ui, "scroll");
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
            Panel::vstack().id_salt("root").show(ui, |ui| {
                Scroll::both()
                    .id_salt("scroll")
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .id_salt("big")
                            .size((Sizing::Fixed(800.0), Sizing::Fixed(800.0)))
                            .show(ui);
                    });
            });
        });
        assert_eq!(
            thumb_rects(&ui, "scroll").len(),
            2,
            "ScrollXY with overflow on both axes should emit two thumbs"
        );
    }

    /// `ScrollXY` with both axes overflowing must NOT have its V and H
    /// bars overlap at the bottom-right corner.
    #[test]
    fn both_axes_bars_dont_overlap_at_corner() {
        let (ui, _node) = record_two_frames(UVec2::new(400, 400), |ui| {
            Panel::vstack().id_salt("root").show(ui, |ui| {
                Scroll::both()
                    .id_salt("scroll")
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .id_salt("big")
                            .size((Sizing::Fixed(800.0), Sizing::Fixed(800.0)))
                            .show(ui);
                    });
            });
        });
        let theme = theme();
        let inner = 200.0 - theme.width - theme.gap;
        let outer_far = 200.0 - theme.width;
        let overlays = thumb_rects(&ui, "scroll");
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
        use crate::primitives::size::Size;
        let surface = UVec2::new(400, 600);
        let mut ui = new_ui();
        let theme = theme();
        let scroll_id = WidgetId::from_hash("scroll").with("__viewport");
        let scene = |ui: &mut Ui| {
            Panel::vstack().id_salt("root").show(ui, |ui| {
                Scroll::vertical()
                    .id_salt("scroll")
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .id_salt("tall")
                            .size((Sizing::Fixed(180.0), Sizing::Fixed(800.0)))
                            .show(ui);
                    });
            });
        };
        ui.run_at_acked(surface, scene);
        let row = *ui.scroll_state(scroll_id);
        let expected = Size::new(200.0 - theme.width - theme.gap, 200.0);
        assert_eq!(
            row.viewport, expected,
            "cold-mount overflowing scroll: gutter reservation must be \
             active on the first painted frame; viewport should already \
             be deflated by `theme.width + theme.gap` on the cross axis",
        );
        assert_eq!(
            row.overflow,
            (false, true),
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
        let mut ui = new_ui();
        let scene = |ui: &mut Ui| {
            Panel::vstack().id_salt("root").show(ui, |ui| {
                Scroll::both()
                    .id_salt("scroll")
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .id_salt("big")
                            .size((Sizing::Fixed(800.0), Sizing::Fixed(800.0)))
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

        ui.run_at_acked(surface, scene);
        let f1 = bar_rects(&ui);
        assert_eq!(f1.len(), 2, "cold-mount must emit both V + H thumbs");

        ui.run_at_acked(surface, scene);
        let f2 = bar_rects(&ui);

        assert_eq!(
            f1, f2,
            "bar shapes on cold-mount frame must match steady-state \
             frame 2 (regression: pass-B used pass-A's stale viewport \
             → bars shrank by theme.width + theme.gap on next frame)",
        );
    }

    /// Cold-mount with content that fits in the viewport: NO gutter
    /// reservation, viewport stays at full outer.
    #[test]
    fn cold_mount_fits_paints_without_gutter_on_first_frame() {
        use crate::primitives::size::Size;
        let surface = UVec2::new(400, 600);
        let mut ui = new_ui();
        let scroll_id = WidgetId::from_hash("scroll").with("__viewport");
        let scene = |ui: &mut Ui| {
            Panel::vstack().id_salt("root").show(ui, |ui| {
                Scroll::vertical()
                    .id_salt("scroll")
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .id_salt("short")
                            .size((Sizing::Fixed(180.0), Sizing::Fixed(50.0)))
                            .show(ui);
                    });
            });
        };
        ui.run_at_acked(surface, scene);
        let row = *ui.scroll_state(scroll_id);
        assert_eq!(
            row.viewport,
            Size::new(200.0, 200.0),
            "cold-mount with no overflow: full outer viewport, no strip",
        );
        assert_eq!(row.overflow, (false, false));
    }
}

/// Press on the V thumb, drag down; `ScrollState.offset.y` moves
/// `delta * (content - viewport) / (track - thumb)` clamped to
/// `[0, content - viewport]`.
#[test]
fn drag_thumb_pans_proportionally() {
    use crate::input::pointer::PointerButton;
    let mut ui = new_ui();
    let build = |ui: &mut Ui| {
        Panel::vstack().id_salt("root").show(ui, |ui| {
            Scroll::vertical()
                .id_salt("scroll")
                .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                .show(ui, |ui| {
                    Frame::new()
                        .id_salt("tall")
                        .size((Sizing::Fixed(180.0), Sizing::Fixed(800.0)))
                        .show(ui);
                });
        });
    };
    ui.run_at_acked(SURFACE, build);
    ui.run_at_acked(SURFACE, build);

    let scroll_id = WidgetId::from_hash("scroll").with("__viewport");
    let thumb_id = scroll_id.with("__vthumb");
    let thumb_rect = ui.response_for(thumb_id).rect.expect("thumb visible");
    let press = thumb_rect.min + Vec2::new(thumb_rect.size.w * 0.5, thumb_rect.size.h * 0.5);

    ui.on_input(InputEvent::PointerMoved(press));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerMoved(press + Vec2::new(0.0, 30.0)));

    ui.run_at_acked(SURFACE, build);

    // viewport = 200, content = 800 ⇒ max_offset = 600.
    // thumb_size = 200 * 200/800 = 50 ⇒ travel = 200 - 50 = 150.
    // factor = 600 / 150 = 4.0 ⇒ offset.y = 30 * 4.0 = 120.
    let offset_y = ui.scroll_state(scroll_id).offset.y;
    assert!(
        (offset_y - 120.0).abs() < 0.5,
        "expected offset.y ≈ 120 after 30 px drag (factor=4), got {offset_y}",
    );

    ui.on_input(InputEvent::PointerMoved(press + Vec2::new(0.0, 9_999.0)));
    ui.run_at_acked(SURFACE, build);
    assert_eq!(
        ui.scroll_state(scroll_id).offset.y,
        600.0,
        "drag past end clamps to max_offset (content - viewport)",
    );
}

#[test]
fn click_on_track_before_thumb_pages_back_after_pages_forward() {
    // Both axes follow the same code path; pin both so the symmetric
    // helper can't drift. For each axis: click far end of track →
    // page forward by one viewport; click near end → page back to 0.
    use crate::input::pointer::PointerButton;
    enum AxisCase {
        V,
        H,
    }
    let cases: &[(&str, AxisCase, &str, &str, f32)] = &[
        ("vertical", AxisCase::V, "scroll", "__vtrack", 200.0),
        ("horizontal", AxisCase::H, "hscroll", "__htrack", 200.0),
    ];
    for (label, axis, scroll_key, track_suffix, page_step) in cases {
        let mut ui = new_ui();
        let build_axis = |ui: &mut Ui| match axis {
            AxisCase::V => build(ui, 200.0, 800.0),
            AxisCase::H => {
                Panel::vstack().id_salt("root").show(ui, |ui| {
                    Scroll::horizontal()
                        .id_salt("hscroll")
                        .size((Sizing::Fixed(200.0), Sizing::Fixed(40.0)))
                        .show(ui, |ui| {
                            Frame::new()
                                .id_salt("hcontent")
                                .size((Sizing::Fixed(800.0), Sizing::Fixed(40.0)))
                                .show(ui);
                        });
                });
            }
        };
        ui.run_at_acked(SURFACE, build_axis);

        let scroll_id = WidgetId::from_hash(*scroll_key).with("__viewport");
        let track_id = scroll_id.with(*track_suffix);
        let track_rect = ui.response_for(track_id).rect.expect("track visible");
        let (forward_press, back_press) = match axis {
            AxisCase::V => (
                track_rect.min + Vec2::new(track_rect.size.w * 0.5, track_rect.size.h - 4.0),
                track_rect.min + Vec2::new(track_rect.size.w * 0.5, 4.0),
            ),
            AxisCase::H => (
                track_rect.min + Vec2::new(track_rect.size.w - 4.0, track_rect.size.h * 0.5),
                track_rect.min + Vec2::new(4.0, track_rect.size.h * 0.5),
            ),
        };

        ui.on_input(InputEvent::PointerMoved(forward_press));
        ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
        ui.on_input(InputEvent::PointerReleased(PointerButton::Left));
        ui.run_at_acked(SURFACE, build_axis);
        let offset = ui.scroll_state(scroll_id).offset;
        let forward = match axis {
            AxisCase::V => offset.y,
            AxisCase::H => offset.x,
        };
        assert_eq!(
            forward, *page_step,
            "case: {label} — click past thumb pages forward by viewport",
        );

        ui.on_input(InputEvent::PointerMoved(back_press));
        ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
        ui.on_input(InputEvent::PointerReleased(PointerButton::Left));
        ui.run_at_acked(SURFACE, build_axis);
        let offset = ui.scroll_state(scroll_id).offset;
        let back = match axis {
            AxisCase::V => offset.y,
            AxisCase::H => offset.x,
        };
        assert_eq!(
            back, 0.0,
            "case: {label} — click before thumb pages back to 0"
        );
    }
}

#[test]
fn ctrl_touchpad_pixel_scroll_zooms_at_same_rate_as_wheel_lines() {
    // The wheel-step refactor split lines vs pixels at the input
    // layer; the zoom path must combine them so a touchpad gesture
    // under ctrl still zooms — pre-split it did, and regressing that
    // breaks touchpad pinch-via-modifier. With line_px = 19.2 (default
    // 16 × 1.2), 38.4 px of touchpad scroll = 2 virtual notches.
    let mut ui = new_ui();
    let build_zoom = |ui: &mut Ui| {
        Panel::vstack().id_salt("root").show(ui, |ui| {
            Scroll::both()
                .id_salt("zoomy")
                .with_zoom()
                .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                .show(ui, |ui| {
                    Frame::new()
                        .id_salt("content")
                        .size((Sizing::Fixed(800.0), Sizing::Fixed(800.0)))
                        .show(ui);
                });
        });
    };
    ui.run_at_acked(SURFACE, build_zoom);

    let scroll_id = WidgetId::from_hash("zoomy").with("__viewport");
    let before_zoom = ui.scroll_state(scroll_id).zoom;

    // Press ctrl, then touchpad-scroll. `wheel_zoom_gate` requires
    // ctrl||cmd; with cfg.step = 1.03 the factor is 1.03^(-2) ≈ 0.9426.
    use crate::input::keyboard::Modifiers;
    ui.on_input(InputEvent::PointerMoved(Vec2::new(100.0, 100.0)));
    ui.on_input(InputEvent::ModifiersChanged(Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    }));
    ui.on_input(InputEvent::ScrollPixels(Vec2::new(0.0, 38.4)));
    ui.run_at_acked(SURFACE, build_zoom);

    let after_zoom = ui.scroll_state(scroll_id).zoom;
    let expected = before_zoom * 1.03_f32.powf(-2.0);
    assert!(
        (after_zoom - expected).abs() < 1e-3,
        "ctrl+touchpad zoom: expected {expected}, got {after_zoom}",
    );
}

#[test]
fn wheel_zoom_step_is_font_independent() {
    // One wheel line = one zoom notch, regardless of theme font size.
    // The line→pan magnitude scales with font; the line→zoom step must
    // not — pin that so a future refactor that reintroduces a
    // font-scaled denominator on the zoom side fails loudly.
    let mut last_zoom: Option<f32> = None;
    for font_size in [12.0_f32, 16.0, 24.0] {
        let mut ui = new_ui();
        ui.theme.text = ui.theme.text.with_font_size(font_size);
        let build_zoom = |ui: &mut Ui| {
            Panel::vstack().id_salt("root").show(ui, |ui| {
                Scroll::both()
                    .id_salt("fz")
                    .with_zoom()
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .id_salt("content")
                            .size((Sizing::Fixed(800.0), Sizing::Fixed(800.0)))
                            .show(ui);
                    });
            });
        };
        ui.run_at_acked(SURFACE, build_zoom);

        use crate::input::keyboard::Modifiers;
        ui.on_input(InputEvent::PointerMoved(Vec2::new(100.0, 100.0)));
        ui.on_input(InputEvent::ModifiersChanged(Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        }));
        ui.on_input(InputEvent::ScrollLines(Vec2::new(0.0, 1.0)));
        ui.run_at_acked(SURFACE, build_zoom);

        let scroll_id = WidgetId::from_hash("fz").with("__viewport");
        let zoom = ui.scroll_state(scroll_id).zoom;
        if let Some(prev) = last_zoom {
            assert!(
                (zoom - prev).abs() < 1e-4,
                "zoom step must be font-independent: prev {prev}, got {zoom} at font_size {font_size}",
            );
        }
        last_zoom = Some(zoom);
    }
}

#[test]
fn line_wheel_step_scales_with_theme_font_size() {
    // Pin: a `ScrollLines(0, 1)` event lands `font_size * line_height_mult`
    // pixels of pan — not the legacy 40 px constant. Two themes, two
    // expected pixel offsets.
    let cases: &[(&str, f32, f32, f32)] = &[
        ("default_16px_text", 16.0, 1.2, 19.2),
        ("larger_24px_text", 24.0, 1.5, 36.0),
    ];
    for (label, font_size, line_height_mult, expected_px) in cases {
        let mut ui = new_ui();
        ui.theme.text = ui
            .theme
            .text
            .with_font_size(*font_size)
            .with_line_height_mult(*line_height_mult);
        let build_v = |ui: &mut Ui| build(ui, 200.0, 800.0);
        ui.run_at_acked(SURFACE, build_v);
        ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
        ui.on_input(InputEvent::ScrollLines(Vec2::new(0.0, 1.0)));
        ui.run_at_acked(SURFACE, build_v);

        let scroll_id = WidgetId::from_hash("scroll").with("__viewport");
        let offset_y = ui.scroll_state(scroll_id).offset.y;
        assert!(
            (offset_y - expected_px).abs() < 0.01,
            "case: {label} — expected {expected_px} px after 1 line wheel, got {offset_y}",
        );
    }
}
