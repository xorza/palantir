use crate::Ui;
use crate::input::InputEvent;
use crate::layout::types::display::Display;
use crate::layout::types::sizing::Sizing;
use crate::primitives::size::Size;
use crate::support::testing::{ui_at, under_outer};
use crate::tree::element::Configure;
use crate::tree::widget_id::WidgetId;
use crate::widgets::frame::Frame;
use crate::widgets::panel::Panel;
use crate::widgets::scroll::{Scroll, ScrollState};
use glam::{UVec2, Vec2};

const SURFACE: UVec2 = UVec2::new(400, 600);

fn surface_display() -> Display {
    Display::from_physical(SURFACE, 1.0)
}

/// Wrap the scroll under a `Panel::vstack` root so its `Sizing::Fixed`
/// is honored. The root expands to surface; the panel's `vstack` slot
/// then hands the scroll exactly its declared size.
fn build(ui: &mut crate::ui::Ui, viewport_h: f32, content_h: f32) {
    Panel::vstack().with_id("root").show(ui, |ui| {
        Scroll::vertical()
            .with_id("scroll")
            .size((Sizing::Fixed(200.0), Sizing::Fixed(viewport_h)))
            .show(ui, |ui| {
                Frame::new()
                    .with_id("content")
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(content_h)))
                    .show(ui);
            });
    });
}

fn read_state(ui: &mut crate::ui::Ui) -> ScrollState {
    *ui.state
        .get_or_insert_with::<ScrollState, _>(WidgetId::from_hash("scroll"), Default::default)
}

#[test]
fn scroll_state_records_viewport_and_content_after_arrange() {
    let mut ui = ui_at(SURFACE);
    build(&mut ui, 200.0, 800.0);
    ui.end_frame();

    let row = read_state(&mut ui);
    assert_eq!(row.viewport.h, 200.0);
    assert_eq!(row.content.h, 800.0);
    assert_eq!(row.offset, Vec2::ZERO, "no wheel input → offset stays at 0");
}

/// Wheel delta accumulates across frames into offset, clamped to
/// `[0, content - viewport]`. When content fits inside the viewport,
/// the offset stays at zero. Each case is a sequence of wheel pushes
/// applied across consecutive frames; the final assertion reads the
/// offset after the last frame, exercising both accumulation and clamp.
#[test]
fn wheel_delta_advances_offset_with_clamp() {
    // (label, viewport_h, content_h, &[wheel_y per frame], expected_final_offset_y)
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
        let mut ui = ui_at(SURFACE);
        build(&mut ui, *viewport_h, *content_h);
        ui.end_frame();

        ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
        for wheel_y in *pushes {
            ui.on_input(InputEvent::Scroll(Vec2::new(0.0, *wheel_y)));
            ui.begin_frame(surface_display());
            build(&mut ui, *viewport_h, *content_h);
            ui.end_frame();
        }

        assert_eq!(read_state(&mut ui).offset.y, *expected, "case: {label}");
    }
}

#[test]
fn horizontal_scroll_pans_only_x() {
    let mut ui = ui_at(SURFACE);
    let build_h = |ui: &mut crate::ui::Ui| {
        Panel::vstack().with_id("root").show(ui, |ui| {
            Scroll::horizontal()
                .with_id("hscroll")
                .size((Sizing::Fixed(200.0), Sizing::Fixed(40.0)))
                .show(ui, |ui| {
                    Frame::new()
                        .with_id("hcontent")
                        .size((Sizing::Fixed(800.0), Sizing::Fixed(40.0)))
                        .show(ui);
                });
        });
    };
    build_h(&mut ui);
    ui.end_frame();

    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 20.0)));
    // Touchpad / wheel deltas come in on both axes — verify only X
    // makes it into the offset for a horizontal scroll.
    ui.on_input(InputEvent::Scroll(Vec2::new(75.0, 200.0)));

    ui.begin_frame(surface_display());
    build_h(&mut ui);
    ui.end_frame();

    let id = WidgetId::from_hash("hscroll");
    let row = *ui
        .state
        .get_or_insert_with::<ScrollState, _>(id, Default::default);
    assert_eq!(row.offset, Vec2::new(75.0, 0.0));
}

#[test]
fn both_axis_scroll_pans_both_axes() {
    let mut ui = ui_at(SURFACE);
    let build_xy = |ui: &mut crate::ui::Ui| {
        Panel::vstack().with_id("root").show(ui, |ui| {
            Scroll::both()
                .with_id("xy")
                .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                .show(ui, |ui| {
                    Frame::new()
                        .with_id("xy-content")
                        .size((Sizing::Fixed(800.0), Sizing::Fixed(800.0)))
                        .show(ui);
                });
        });
    };
    build_xy(&mut ui);
    ui.end_frame();

    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    ui.on_input(InputEvent::Scroll(Vec2::new(40.0, 60.0)));

    ui.begin_frame(surface_display());
    build_xy(&mut ui);
    ui.end_frame();

    let id = WidgetId::from_hash("xy");
    let row = *ui
        .state
        .get_or_insert_with::<ScrollState, _>(id, Default::default);
    assert_eq!(row.offset, Vec2::new(40.0, 60.0));
    assert_eq!(
        row.content,
        crate::primitives::size::Size::new(800.0, 800.0)
    );
    // Viewport is the inner (post-padding) area. Scroll reserves
    // `theme.width + theme.gap = 12px` on each panned axis when
    // content overflows, so the 200×200 outer rect leaves a 188×188
    // inner region.
    assert_eq!(
        row.viewport,
        crate::primitives::size::Size::new(188.0, 188.0)
    );
}

// --- Measure-side: scroll_content captured by the layout pass ---------------
// Pin the contract between the `Scroll(axes)` arm of `measure_dispatch` and
// `LayoutResult.scroll_content`: content extent lands there, the viewport's
// own desired stays at zero on the panned axes (so `resolve_desired` falls
// through to the user's `Sizing`).

/// `LayoutResult.scroll_content` records the content extent the
/// scroll viewport sees. V-axis and H-axis behave like a Stack: sum
/// along the panned axis, max on the cross. XY behaves like a ZStack:
/// max per axis. An empty scroll records zero.
#[test]
fn scroll_records_content_extent() {
    enum Axis {
        V,
        H,
        XY,
        Empty,
    }
    let cases: &[(&str, Axis, Size)] = &[
        ("v_axis_sum_main_max_cross", Axis::V, Size::new(180.0, 92.0)),
        ("h_axis_sum_main_max_cross", Axis::H, Size::new(128.0, 40.0)),
        ("xy_max_per_axis", Axis::XY, Size::new(300.0, 250.0)),
        ("empty_records_zero", Axis::Empty, Size::ZERO),
    ];
    for (label, axis, expected) in cases {
        let mut ui = Ui::new();
        let surface = match axis {
            Axis::V | Axis::Empty => UVec2::new(400, 600),
            Axis::H => UVec2::new(800, 200),
            Axis::XY => UVec2::new(400, 400),
        };
        let scroll_node = under_outer(&mut ui, surface, |ui| {
            match axis {
                Axis::V => {
                    Scroll::vertical()
                        .with_id("scroll")
                        .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                        .gap(4.0)
                        .show(ui, |ui| {
                            // 3 rows of 28h, 4px gap → 28*3 + 4*2 = 92.
                            for i in 0..3u32 {
                                Frame::new()
                                    .with_id(("row", i))
                                    .size((Sizing::Fixed(180.0), Sizing::Fixed(28.0)))
                                    .show(ui);
                            }
                        })
                        .node
                }
                Axis::H => {
                    Scroll::horizontal()
                        .with_id("scroll")
                        .size((Sizing::Fixed(200.0), Sizing::Fixed(60.0)))
                        .gap(8.0)
                        .show(ui, |ui| {
                            // 2 cols of 60w, 8px gap → 60*2 + 8 = 128.
                            for i in 0..2u32 {
                                Frame::new()
                                    .with_id(("col", i))
                                    .size((Sizing::Fixed(60.0), Sizing::Fixed(40.0)))
                                    .show(ui);
                            }
                        })
                        .node
                }
                Axis::XY => {
                    Scroll::both()
                        .with_id("scroll")
                        .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
                        .show(ui, |ui| {
                            Frame::new()
                                .with_id("wide")
                                .size((Sizing::Fixed(300.0), Sizing::Fixed(60.0)))
                                .show(ui);
                            Frame::new()
                                .with_id("tall")
                                .size((Sizing::Fixed(80.0), Sizing::Fixed(250.0)))
                                .show(ui);
                        })
                        .node
                }
                Axis::Empty => {
                    Scroll::vertical()
                        .with_id("empty")
                        .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
                        .show(ui, |_| {})
                        .node
                }
            }
        });
        let rect = ui.pipeline.layout.result.rect[scroll_node.index()];
        let content = ui.pipeline.layout.result.scroll_content[scroll_node.index()];
        assert_eq!(content, *expected, "case: {label} content");
        // Viewport honors the Scroll's Fixed size, ignoring overflow content.
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

#[test]
fn scroll_content_survives_measure_cache_hit() {
    // Two frames with identical input: first frame populates
    // `scroll_content` from the live measure; second frame's measure
    // cache short-circuits an ancestor and must restore `scroll_content`
    // verbatim from the snapshot. Pins that the cache plumbing carries
    // the column rather than re-deriving it.
    let surface = UVec2::new(400, 600);
    let display = Display::from_physical(surface, 1.0);
    let build = |ui: &mut Ui| {
        Panel::vstack().with_id("root").show(ui, |ui| {
            Scroll::vertical()
                .with_id("scroll")
                .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                .gap(4.0)
                .show(ui, |ui| {
                    for i in 0..3u32 {
                        Frame::new()
                            .with_id(("row", i))
                            .size((Sizing::Fixed(180.0), Sizing::Fixed(28.0)))
                            .show(ui);
                    }
                });
        });
    };

    let mut ui = ui_at(surface);
    build(&mut ui);
    ui.end_frame();
    let scroll_id = WidgetId::from_hash("scroll");
    let after_first = *ui
        .state
        .get_or_insert_with::<ScrollState, _>(scroll_id, Default::default);
    assert_eq!(after_first.content.h, 92.0);

    ui.begin_frame(display);
    build(&mut ui);
    ui.end_frame();
    let after_second = *ui
        .state
        .get_or_insert_with::<ScrollState, _>(scroll_id, Default::default);
    assert_eq!(
        after_second.content, after_first.content,
        "scroll_content survives a measure-cache hit"
    );
    assert_eq!(after_second.viewport, after_first.viewport);
}

// --- Scrollbar geometry ----------------------------------------------------
// Pin the formulas in `scroll::bar_geometry` against the design-doc math
// and pin that bar shapes actually land on the scroll node when content
// overflows.

mod bars {
    use crate::Ui;
    use crate::layout::types::display::Display;
    use crate::layout::types::sizing::Sizing;
    use crate::shape::Shape;
    use crate::support::testing::ui_at;
    use crate::tree::NodeId;
    use crate::tree::element::Configure;
    use crate::tree::widget_id::WidgetId;
    use crate::widgets::frame::Frame;
    use crate::widgets::panel::Panel;
    use crate::widgets::scroll::{Scroll, bar_geometry};
    use crate::widgets::theme::{Background, ScrollbarTheme, Surface};
    use glam::UVec2;

    fn theme() -> ScrollbarTheme {
        ScrollbarTheme::default()
    }

    /// `bar_geometry(viewport, content, offset, track, theme)` returns
    /// `None` when content fits the viewport or the track collapses to
    /// zero; otherwise `Some { thumb_size, thumb_offset }`. `thumb_size`
    /// = (viewport / content) * track, clamped to `[min_thumb_px, track]`.
    /// `thumb_offset` rides linearly with `offset` and reaches `track -
    /// thumb_size` at max offset.
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
                // travel = track - thumb_size = 180 - 45 = 135.
                // offset/max = 300/600 = 0.5 → thumb_offset = 0.5 * 135 = 67.5.
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

    /// Build a scroll over two frames so end_frame settles `ScrollState`
    /// before the bar-emit check on frame 2.
    fn record_two_frames<F: Fn(&mut Ui) + Copy>(surface: UVec2, build: F) -> (Ui, NodeId) {
        let mut ui = ui_at(surface);
        build(&mut ui);
        ui.end_frame();
        ui.begin_frame(Display::from_physical(surface, 1.0));
        build(&mut ui);
        let scroll_id = WidgetId::from_hash("scroll");
        let idx = ui
            .tree
            .records
            .widget_id()
            .iter()
            .position(|w| *w == scroll_id)
            .expect("scroll widget recorded");
        (ui, NodeId(idx as u32))
    }

    fn count_positioned(ui: &Ui, node: NodeId) -> usize {
        ui.tree
            .shapes_of(node)
            .filter(|s| matches!(s, Shape::SubRect { .. }))
            .count()
    }

    #[test]
    fn vertical_overflow_emits_thumb_shape_after_settle() {
        let (ui, node) = record_two_frames(UVec2::new(400, 600), |ui| {
            Panel::vstack().with_id("root").show(ui, |ui| {
                Scroll::vertical()
                    .with_id("scroll")
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .with_id("tall")
                            .size((Sizing::Fixed(180.0), Sizing::Fixed(800.0)))
                            .show(ui);
                    });
            });
        });
        assert!(
            count_positioned(&ui, node) >= 1,
            "vertical overflow should emit at least one bar shape"
        );
    }

    #[test]
    fn no_bar_when_content_fits_viewport() {
        let (ui, node) = record_two_frames(UVec2::new(400, 400), |ui| {
            Panel::vstack().with_id("root").show(ui, |ui| {
                Scroll::vertical()
                    .with_id("scroll")
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .with_id("short")
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
    /// with a Scroll that emits bar shapes. If the encoder's two-phase
    /// shape emission corrupts the cmd stream's clip balance, compose
    /// panics here.
    #[test]
    fn scroll_with_bars_composes_through_warm_cache() {
        let surface = UVec2::new(400, 300);
        let mut ui = ui_at(surface);
        let build = |ui: &mut Ui| {
            Panel::vstack().with_id("root").show(ui, |ui| {
                Scroll::vertical()
                    .with_id("scroll")
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                    .show(ui, |ui| {
                        for i in 0..30u32 {
                            Frame::new()
                                .with_id(("row", i))
                                .size((Sizing::Fixed(180.0), Sizing::Fixed(28.0)))
                                .show(ui);
                        }
                    });
            });
        };
        build(&mut ui);
        ui.end_frame();
        // Frame 2 — caches warm; this is what panicked in the showcase.
        crate::support::testing::begin(&mut ui, surface);
        build(&mut ui);
        ui.end_frame();
    }

    /// Showcase-style nested scroll cards (Scroll inside a clipped Panel
    /// inside a vstack). Pin that the deeper clip-stack walk + warm
    /// caches still leave the cmd stream balanced.
    #[test]
    fn nested_clipped_scrolls_compose_through_warm_cache() {
        let surface = UVec2::new(800, 600);
        let mut ui = ui_at(surface);
        let build = |ui: &mut Ui| {
            Panel::hstack()
                .with_id("root")
                .gap(12.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    for tag in ["v", "h", "xy"] {
                        Panel::vstack()
                            .with_id(("card", tag))
                            .padding(8.0)
                            .size((Sizing::FILL, Sizing::FILL))
                            .background(Surface::clip_rect_with_bg(Background {
                                fill: crate::primitives::color::Color::rgb(0.16, 0.20, 0.28),
                                ..Default::default()
                            }))
                            .show(ui, |ui| {
                                let s = match tag {
                                    "v" => Scroll::vertical().with_id(("scroll", tag)),
                                    "h" => Scroll::horizontal().with_id(("scroll", tag)),
                                    _ => Scroll::both().with_id(("scroll", tag)),
                                };
                                s.size((Sizing::FILL, Sizing::FILL)).show(ui, |ui| {
                                    for i in 0..40u32 {
                                        Frame::new()
                                            .with_id((tag, "item", i))
                                            .size((Sizing::Fixed(120.0), Sizing::Fixed(28.0)))
                                            .show(ui);
                                    }
                                });
                            });
                    }
                });
        };
        build(&mut ui);
        ui.end_frame();
        crate::support::testing::begin(&mut ui, surface);
        build(&mut ui);
        ui.end_frame();
        crate::support::testing::begin(&mut ui, surface);
        build(&mut ui);
        ui.end_frame();
    }

    /// Reservation: when content overflows on the V axis, the inner
    /// (viewport) shrinks by exactly `theme.width` on the right.
    /// Frame 1 records with no overflow yet (state row zero), frame 2
    /// reserves once `end_frame` settles `content > viewport`.
    #[test]
    fn vertical_overflow_reserves_bar_width_on_inner() {
        use crate::primitives::size::Size;
        use crate::widgets::scroll::ScrollState;
        let surface = UVec2::new(400, 600);
        let mut ui = ui_at(surface);
        let build = |ui: &mut Ui| {
            Panel::vstack().with_id("root").show(ui, |ui| {
                Scroll::vertical()
                    .with_id("scroll")
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .with_id("tall")
                            .size((Sizing::Fixed(180.0), Sizing::Fixed(800.0)))
                            .show(ui);
                    });
            });
        };
        build(&mut ui);
        ui.end_frame();
        crate::support::testing::begin(&mut ui, surface);
        build(&mut ui);
        ui.end_frame();
        let row = *ui
            .state
            .get_or_insert_with::<ScrollState, _>(WidgetId::from_hash("scroll"), Default::default);
        assert_eq!(
            row.viewport,
            Size::new(188.0, 200.0),
            "V overflow reserves theme.width + theme.gap = 12px on the right; H axis untouched"
        );
    }

    /// User-set padding is preserved — bar reservation adds to it
    /// rather than replacing. 16px right + 8px reservation = 24px.
    #[test]
    fn user_padding_is_preserved_when_bar_reserves() {
        use crate::primitives::size::Size;
        use crate::widgets::scroll::ScrollState;
        let surface = UVec2::new(400, 600);
        let mut ui = ui_at(surface);
        let build = |ui: &mut Ui| {
            Panel::vstack().with_id("root").show(ui, |ui| {
                Scroll::vertical()
                    .with_id("scroll")
                    .padding(16.0)
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .with_id("tall")
                            .size((Sizing::Fixed(100.0), Sizing::Fixed(800.0)))
                            .show(ui);
                    });
            });
        };
        build(&mut ui);
        ui.end_frame();
        crate::support::testing::begin(&mut ui, surface);
        build(&mut ui);
        ui.end_frame();
        let row = *ui
            .state
            .get_or_insert_with::<ScrollState, _>(WidgetId::from_hash("scroll"), Default::default);
        // Inner x = 200 - (left=16 + right=16 + reservation=8+4) = 156.
        // Inner y = 200 - (top=16 + bottom=16) = 168.
        assert_eq!(row.viewport, Size::new(156.0, 168.0));
    }

    /// Pin bar positioning: V bar's overlay rect sits flush with
    /// `outer.w - theme.width` (the reserved padding strip), NOT
    /// inside any user-set padding. Specifically pins the
    /// user-padding case — using `viewport.w` (= inner) for the bar
    /// position would put the bar at x = inner.w which falls inside
    /// the user padding region instead of the reserved strip.
    #[test]
    fn vertical_bar_overlay_rect_lands_in_right_padding_strip() {
        // 200x200 outer, user padding 16 all sides + 8 reservation right
        // ⇒ inner = (200 - 16 - 16 - 8, 200 - 32) = (160, 168).
        // Bar should sit at x = outer.w - theme.width = 200 - 8 = 192,
        // NOT at viewport.w = 160 (which would overlap user padding).
        let (ui, node) = record_two_frames(UVec2::new(400, 600), |ui| {
            Panel::vstack().with_id("root").show(ui, |ui| {
                Scroll::vertical()
                    .with_id("scroll")
                    .padding(16.0)
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .with_id("tall")
                            .size((Sizing::Fixed(100.0), Sizing::Fixed(800.0)))
                            .show(ui);
                    });
            });
        });
        let theme = theme();
        let expected_x = 200.0 - theme.width;
        let overlays: Vec<_> = ui
            .tree
            .shapes_of(node)
            .filter_map(|s| match s {
                Shape::SubRect {
                    local_rect: rect, ..
                } => Some(*rect),
                _ => None,
            })
            .collect();
        assert!(
            !overlays.is_empty(),
            "expected at least one overlay shape (thumb)"
        );
        for r in &overlays {
            assert_eq!(
                r.min.x, expected_x,
                "V bar must sit at outer.w - theme.width (= reserved strip), \
                 not inside user padding"
            );
            assert_eq!(r.size.w, theme.width, "V bar width = theme.width");
        }
    }

    /// Reservation must collapse when overflow goes away. Frame 1
    /// records overflowing content (state row zero, no padding yet).
    /// Frame 2 settles overflow → reserves padding (viewport shrinks).
    /// Frame 3 swaps to short content → still reserves (last frame
    /// said overflow). Frame 4 sees no-overflow on inner → padding
    /// drops back to zero, viewport returns to outer size.
    #[test]
    fn bar_reservation_collapses_when_overflow_disappears() {
        use crate::primitives::size::Size;
        use crate::widgets::scroll::ScrollState;
        let surface = UVec2::new(400, 600);
        let scroll_id = WidgetId::from_hash("scroll");
        let read_viewport = |ui: &mut Ui| {
            ui.state
                .get_or_insert_with::<ScrollState, _>(scroll_id, Default::default)
                .viewport
        };

        let build = |ui: &mut Ui, content_h: f32| {
            Panel::vstack().with_id("root").show(ui, |ui| {
                Scroll::vertical()
                    .with_id("scroll")
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .with_id("body")
                            .size((Sizing::Fixed(180.0), Sizing::Fixed(content_h)))
                            .show(ui);
                    });
            });
        };

        // Two frames with overflow → reservation kicks in.
        let mut ui = ui_at(surface);
        build(&mut ui, 800.0);
        ui.end_frame();
        crate::support::testing::begin(&mut ui, surface);
        build(&mut ui, 800.0);
        ui.end_frame();
        assert_eq!(
            read_viewport(&mut ui),
            Size::new(188.0, 200.0),
            "frame 2: reservation active, viewport = 200 - (width + gap)"
        );

        // Swap to short content. Frame 3 still reserves (decision made
        // from frame 2's state). Frame 4 sees no-overflow on the inner
        // and drops the reservation.
        crate::support::testing::begin(&mut ui, surface);
        build(&mut ui, 50.0);
        ui.end_frame();
        crate::support::testing::begin(&mut ui, surface);
        build(&mut ui, 50.0);
        ui.end_frame();
        assert_eq!(
            read_viewport(&mut ui),
            Size::new(200.0, 200.0),
            "after content shrinks, reservation collapses; viewport = full outer"
        );
    }

    #[test]
    fn both_axes_overflow_emits_two_thumbs() {
        let (ui, node) = record_two_frames(UVec2::new(400, 400), |ui| {
            Panel::vstack().with_id("root").show(ui, |ui| {
                Scroll::both()
                    .with_id("scroll")
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .with_id("big")
                            .size((Sizing::Fixed(800.0), Sizing::Fixed(800.0)))
                            .show(ui);
                    });
            });
        });
        // Default theme has transparent track → only thumbs land as
        // Overlay, one per axis.
        assert_eq!(
            count_positioned(&ui, node),
            2,
            "ScrollXY with overflow on both axes should emit two thumbs"
        );
    }

    /// `ScrollXY` with both axes overflowing must NOT have its V and H
    /// bars overlap at the bottom-right corner. V bar's main extent
    /// (height) ends at `viewport.h` (= inner h, excludes the bottom
    /// reserved strip); H bar's main extent (width) ends at
    /// `viewport.w`. Pin both via the emitted Overlay rects.
    #[test]
    fn both_axes_bars_dont_overlap_at_corner() {
        let (ui, node) = record_two_frames(UVec2::new(400, 400), |ui| {
            Panel::vstack().with_id("root").show(ui, |ui| {
                Scroll::both()
                    .with_id("scroll")
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .with_id("big")
                            .size((Sizing::Fixed(800.0), Sizing::Fixed(800.0)))
                            .show(ui);
                    });
            });
        });
        let theme = theme();
        // Both axes reserve theme.width + theme.gap → inner viewport
        // = 188 × 188, outer = 200 × 200. Bar position (cross axis)
        // sits flush with outer's far edge minus theme.width — the
        // gap is the empty strip between content and bar.
        let inner = 200.0 - theme.width - theme.gap;
        let outer_far = 200.0 - theme.width; // bar.cross_pos
        let overlays: Vec<_> = ui
            .tree
            .shapes_of(node)
            .filter_map(|s| match s {
                Shape::SubRect {
                    local_rect: rect, ..
                } => Some(*rect),
                _ => None,
            })
            .collect();
        assert_eq!(overlays.len(), 2, "expected V + H thumbs");
        // V thumb: x at outer_far, max.y ≤ inner (doesn't enter H strip).
        // H thumb: y at outer_far, max.x ≤ inner (doesn't enter V strip).
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
}
