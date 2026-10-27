//! What moves the offset, and where it is clamped.

use crate::input::scroll_targets::ScrollTargets;
use crate::internals::ui_harness::UiHarness;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use crate::widgets::panel::Panel;
use crate::widgets::scroll::Scroll;
use crate::widgets::scroll::scroll_state::ScrollState;
use crate::widgets::scroll::tests::support::{
    SURFACE, build, driven, fixed_block, read_state, scroll_viewport,
};
use glam::Vec2;

/// Wheel delta accumulates into the offset, clamped to `[0, content - viewport]`;
/// content that fits stays at zero.
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
        (
            "non_overflowing_positive_wheel_stays_zero",
            300.0,
            100.0,
            &[500.0],
            0.0,
        ),
        (
            "non_overflowing_negative_wheel_stays_zero",
            300.0,
            100.0,
            &[-500.0],
            0.0,
        ),
    ];
    for (label, viewport_h, content_h, pushes, expected) in cases {
        let mut h = UiHarness::new(SURFACE);
        h.frame(|ui| build(ui, *viewport_h, *content_h));
        // Checked aim: a missed wheel would also leave the non-overflowing rows at zero.
        h.move_onto(WidgetId::from_hash("scroll"));
        for wheel_y in *pushes {
            h.scroll_pixels(Vec2::new(0.0, *wheel_y));
            h.frame(|ui| build(ui, *viewport_h, *content_h));
        }

        assert_eq!(read_state(&h).offset().y, *expected, "case: {label}");
    }
}

/// `content_margin` shifts the offset range off `[0, slack]`: a left/top margin
/// `m` opens a `[-m, 0)` band to pan past the origin, and the right/bottom
/// margin extends the upper bound by the same amount.
#[test]
fn content_margin_allows_negative_pan_into_left_top_band() {
    let mut h = UiHarness::new(SURFACE);
    let m = 100.0;
    let build_m = |ui: &mut Ui| {
        Scroll::both()
            .id(WidgetId::from_hash("scroll"))
            .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
            .hide_bars()
            .content_margin(m)
            .show(ui, |ui| {
                fixed_block(ui, WidgetId::from_hash("content"), 400.0, 400.0);
            });
    };
    h.frame(build_m);
    h.move_to(Vec2::new(50.0, 50.0));
    // Pan left/up: a large negative delta clamps at `-m` on both axes.
    h.scroll_pixels(Vec2::new(-9_999.0, -9_999.0));
    h.frame(build_m);
    assert_eq!(read_state(&h).offset(), Vec2::new(-m, -m));
    // Pan back: clamp at `raw_slack + m` = (400 - 200) + 100 = 300.
    h.scroll_pixels(Vec2::new(9_999.0, 9_999.0));
    h.frame(build_m);
    let raw_slack = 400.0 - 200.0;
    assert_eq!(
        read_state(&h).offset(),
        Vec2::new(raw_slack + m, raw_slack + m)
    );
}

/// A wheel pans the axes a scroll pans and no other, and a scroll disabled on the wheel's frame
/// pans nothing, though its response read the wheel a frame before the cascade knew. A panned
/// axis loses the Reserved gutter's `theme.thickness + theme.gap = 12` px across it, overflow or
/// not, so `both`'s 200 px box shows 188.
#[test]
fn a_wheel_pans_only_the_scrolled_axes() {
    #[derive(Debug)]
    struct Case {
        label: &'static str,
        scroll: fn() -> Scroll<'static>,
        size: Vec2,
        disabled: bool,
        offset: Vec2,
        viewport: Size,
    }
    let cases = [
        Case {
            label: "horizontal",
            scroll: Scroll::horizontal,
            size: Vec2::new(200.0, 40.0),
            disabled: false,
            offset: Vec2::new(75.0, 0.0),
            viewport: Size::new(200.0, 28.0),
        },
        Case {
            label: "both",
            scroll: Scroll::both,
            size: Vec2::new(200.0, 200.0),
            disabled: false,
            offset: Vec2::new(75.0, 200.0),
            viewport: Size::new(188.0, 188.0),
        },
        Case {
            label: "both, disabled on the wheel's frame",
            scroll: Scroll::both,
            size: Vec2::new(200.0, 200.0),
            disabled: true,
            offset: Vec2::ZERO,
            viewport: Size::new(188.0, 188.0),
        },
    ];
    let id = WidgetId::from_hash("scrolled");
    for case in cases {
        let build = |ui: &mut Ui, disabled: bool| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |ui| {
                    (case.scroll)()
                        .id(id)
                        .size((Sizing::fixed(case.size.x), Sizing::fixed(case.size.y)))
                        .disabled(disabled)
                        .show(ui, |ui| {
                            fixed_block(ui, WidgetId::from_hash("content"), 800.0, 800.0);
                        });
                });
        };
        let mut h = UiHarness::new(SURFACE);
        h.frame(|ui| build(ui, false));
        h.scroll_pixels_at(Vec2::new(50.0, 20.0), Vec2::new(75.0, 200.0));
        h.frame(|ui| build(ui, case.disabled));
        assert_eq!(
            h.state::<ScrollState>(id).offset(),
            case.offset,
            "{}",
            case.label
        );
        assert_eq!(scroll_viewport(&h.ui, id), case.viewport, "{}", case.label);
    }
}

/// Press on the V thumb and drag down: `ScrollState.offset().y` moves
/// `delta * (content - viewport) / (track - thumb)`, clamped to
/// `[0, content - viewport]`.
#[test]
fn drag_thumb_pans_proportionally() {
    for scale in [0.5, 1.0, 2.0] {
        let mut h = UiHarness::new(SURFACE);
        let build = |ui: &mut Ui| {
            Panel::zstack()
                .id(WidgetId::from_hash("scaled-scrollbar-parent"))
                .transform(TranslateScale::from_scale(scale))
                .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                .show(ui, |ui| {
                    Scroll::vertical()
                        .id(WidgetId::from_hash("scroll"))
                        .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                        .show(ui, |ui| {
                            fixed_block(ui, WidgetId::from_hash("tall"), 180.0, 800.0);
                        });
                });
        };
        h.prime(2, build);

        let outer_id = WidgetId::from_hash("scroll");
        let scroll_id = outer_id.with("viewport");
        let thumb_id = scroll_id.with("vthumb");
        let press = h.center_of(thumb_id);
        h.press_on(thumb_id);
        h.move_to(press + Vec2::new(0.0, 30.0 * scale));
        h.frame(build);

        // viewport = 200, content = 800 ⇒ max_offset = 600.
        // thumb_size = 200 * 200/800 = 50 ⇒ travel = 200 - 50 = 150.
        // factor = 600 / 150 = 4.0 ⇒ offset.y = 30 * 4.0 = 120.
        let offset_y = h.state::<ScrollState>(outer_id).offset().y;
        assert_eq!(
            offset_y, 120.0,
            "30 logical px at {scale}× should produce offset 120, got {offset_y}"
        );

        h.move_to(press + Vec2::new(0.0, 9_999.0 * scale));
        h.frame(build);
        assert_eq!(
            h.state::<ScrollState>(outer_id).offset().y,
            600.0,
            "drag past end at {scale}× clamps to max offset",
        );
    }
}

#[test]
fn click_on_track_before_thumb_pages_back_after_pages_forward() {
    // Both axes share a code path; pin both so the symmetric helper can't drift.
    // Click the far end of the track to page forward one viewport; the near end to
    // page back to 0.
    #[derive(Debug)]
    enum AxisCase {
        V,
        H,
    }
    let cases: &[(&str, AxisCase, &str, &str, f32)] = &[
        ("vertical", AxisCase::V, "scroll", "vtrack", 200.0),
        ("horizontal", AxisCase::H, "hscroll", "htrack", 200.0),
    ];
    for scale in [0.5, 1.0, 2.0] {
        for (label, axis, scroll_key, track_suffix, page_step) in cases {
            let mut h = UiHarness::new(SURFACE);
            let build_axis = |ui: &mut Ui| {
                Panel::zstack()
                    .id(WidgetId::from_hash("scaled-track-parent"))
                    .transform(TranslateScale::from_scale(scale))
                    .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                    .show(ui, |ui| match axis {
                        AxisCase::V => build(ui, 200.0, 800.0),
                        AxisCase::H => {
                            Panel::vstack()
                                .id(WidgetId::from_hash("root"))
                                .show(ui, |ui| {
                                    Scroll::horizontal()
                                        .id(WidgetId::from_hash("hscroll"))
                                        .size((Sizing::fixed(200.0), Sizing::fixed(40.0)))
                                        .show(ui, |ui| {
                                            fixed_block(
                                                ui,
                                                WidgetId::from_hash("hcontent"),
                                                800.0,
                                                40.0,
                                            );
                                        });
                                });
                        }
                    });
            };
            h.frame(build_axis);

            let outer_id = WidgetId::from_hash(*scroll_key);
            let scroll_id = outer_id.with("viewport");
            let track_id = scroll_id.with(*track_suffix);
            let (forward_local, back_local) = match axis {
                AxisCase::V => (Vec2::new(6.0, 196.0), Vec2::new(6.0, 4.0)),
                AxisCase::H => (Vec2::new(196.0, 6.0), Vec2::new(4.0, 6.0)),
            };
            let forward_press = h.point_in(track_id, forward_local);

            h.press_at(forward_press);
            h.release();
            h.frame(build_axis);
            let offset = h.state::<ScrollState>(outer_id).offset();
            let forward = match axis {
                AxisCase::V => offset.y,
                AxisCase::H => offset.x,
            };
            assert_eq!(
                forward, *page_step,
                "case: {label} at {scale}× — click past thumb pages forward",
            );

            // Aimed after the forward page; before it the thumb sat over this end.
            let back_press = h.point_in(track_id, back_local);
            h.press_at(back_press);
            h.release();
            h.frame(build_axis);
            let offset = h.state::<ScrollState>(outer_id).offset();
            let back = match axis {
                AxisCase::V => offset.y,
                AxisCase::H => offset.x,
            };
            assert_eq!(
                back, 0.0,
                "case: {label} at {scale}× — click before thumb pages back",
            );
        }
    }
}

/// Regression: a non-zoomable scroll must pull its offset back into range when
/// content *shrinks* with no wheel/drag input, else the viewport is stranded in
/// the empty tail. The record-time clamp reads the previous frame's arranged
/// content, so the correction lands a frame after the shrink's arrange; it used
/// to run only on nonzero pan input.
#[test]
fn shrinking_content_unstrands_offset_without_input() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| build(ui, 200.0, 800.0));
    h.scroll_pixels_at(Vec2::new(50.0, 50.0), Vec2::new(0.0, 10_000.0));
    h.frame(|ui| build(ui, 200.0, 800.0));
    assert_eq!(
        read_state(&h).offset().y,
        600.0,
        "precondition: scrolled to max (800 - 200)",
    );

    // Content shrinks to 300px (new max = 100), NO input. Frame 1 records against
    // the stale 800px content (offset stays 600) and arranges the new content;
    // frame 2 records against it and clamps the stranded offset.
    h.prime(2, |ui| build(ui, 200.0, 300.0));
    assert_eq!(
        read_state(&h).offset().y,
        100.0,
        "offset must clamp to the new max (300 - 200) after a passive content shrink",
    );
}

/// Content that fits its viewport rests at offset zero even when a leading
/// `content_margin` opens a band below it. The margin is invisible overscroll
/// that leaves layout alone, so it may only widen the pannable range, never
/// move content at rest.
#[test]
fn content_margin_does_not_shift_content_that_fits() {
    let mut h = UiHarness::new(SURFACE);
    let m = 100.0;
    let build = |ui: &mut Ui| {
        Scroll::both()
            .id(WidgetId::from_hash("scroll"))
            .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
            .hide_bars()
            .content_margin(m)
            .show(ui, |ui| {
                // Smaller than the 200x200 viewport: no overflow, so rest offset must be zero.
                Block::new()
                    .id(WidgetId::from_hash("content"))
                    .size((Sizing::fixed(80.0), Sizing::fixed(80.0)))
                    .show(ui);
            });
    };
    h.prime(2, build);
    assert_eq!(
        read_state(&h).offset(),
        Vec2::ZERO,
        "fitting content must rest at the origin, not at the leading margin",
    );

    // The margin still opens its band: the user can pan into it and back out to
    // rest.
    h.move_to(Vec2::new(50.0, 50.0));
    h.scroll_pixels(Vec2::new(-9_999.0, -9_999.0));
    h.frame(build);
    assert_eq!(
        read_state(&h).offset(),
        Vec2::new(-m, -m),
        "the leading band is still reachable for content that fits",
    );
    h.scroll_pixels(Vec2::new(9_999.0, 9_999.0));
    h.frame(build);
    assert_eq!(
        read_state(&h).offset(),
        Vec2::new(m, m),
        "and so is the trailing band",
    );
}

/// [`Scroll::pan_by`] takes the step a wheel of that distance takes, through the
/// same clamp, with no pointer over the viewport, so authoring code can drive a
/// view. 200 of viewport over 800 of content: offset reaches 600 and no more.
#[test]
fn pan_by_pans_without_a_pointer_and_clamps() {
    let drive = |ui: &mut Ui, delta: f32| driven(ui, 200.0, 800.0, Vec2::new(0.0, delta));
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| drive(ui, 0.0));
    assert_eq!(
        read_state(&h).offset().y,
        0.0,
        "premise: the viewport starts at the top",
    );

    h.frame(|ui| drive(ui, 50.0));
    assert_eq!(read_state(&h).offset().y, 50.0, "one request, one step");

    h.frame(|ui| drive(ui, 50.0));
    assert_eq!(
        read_state(&h).offset().y,
        100.0,
        "and the steps accumulate, as a wheel's do",
    );

    h.frame(|ui| drive(ui, 9_999.0));
    assert_eq!(
        read_state(&h).offset().y,
        600.0,
        "clamped at content minus viewport, like every other pan",
    );
}

/// Everything asking to pan one frame contributes to one step: two builder
/// requests plus a wheel; none replaces another.
#[test]
fn pan_by_composes_with_a_wheel_and_with_itself() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| driven(ui, 200.0, 800.0, Vec2::ZERO));
    h.move_to(Vec2::new(50.0, 50.0));

    h.scroll_pixels(Vec2::new(0.0, 30.0));
    h.frame(|ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Scroll::vertical()
                    .id(WidgetId::from_hash("scroll"))
                    .pan_by(Vec2::new(0.0, 12.0))
                    .pan_by(Vec2::new(0.0, 8.0))
                    .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                    .show(ui, |ui| {
                        fixed_block(ui, WidgetId::from_hash("content"), 200.0, 800.0);
                    });
            });
    });
    assert_eq!(
        read_state(&h).offset().y,
        50.0,
        "30 px of wheel and two requests of 12 and 8 are 50 px of pan",
    );
}

/// A viewport senses the wheel only on axes it can pan, so the other axis
/// reaches the container behind. Before its first arrange it claims every axis
/// it declares; a zoomable one always claims both (its wheel zooms).
#[test]
fn a_viewport_senses_the_wheel_only_on_the_axes_it_can_pan() {
    type Case = (
        &'static str,
        fn() -> Scroll<'static>,
        (f32, f32),
        ScrollTargets,
        ScrollTargets,
    );
    let id = WidgetId::from_hash("sensing");
    let both = ScrollTargets::both(id);
    let x_only = ScrollTargets {
        x: Some(id),
        y: None,
    };
    let y_only = ScrollTargets {
        x: None,
        y: Some(id),
    };
    let none = ScrollTargets::default();
    // (case, the viewport, its content, targets after one frame, after two)
    let cases: [Case; 6] = [
        (
            "vertical, overflowing",
            Scroll::vertical,
            (100.0, 800.0),
            y_only,
            y_only,
        ),
        (
            "vertical, fitting",
            Scroll::vertical,
            (100.0, 100.0),
            y_only,
            none,
        ),
        (
            "both, wide content",
            Scroll::both,
            (800.0, 100.0),
            both,
            x_only,
        ),
        (
            "both, tall content",
            Scroll::both,
            (100.0, 800.0),
            both,
            y_only,
        ),
        ("both, fitting", Scroll::both, (100.0, 100.0), both, none),
        (
            "zoomable, fitting",
            || Scroll::both().zoomable(),
            (100.0, 100.0),
            both,
            both,
        ),
    ];
    for (label, scroll, (w, h_content), first, settled) in cases {
        let build = |ui: &mut Ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |ui| {
                    scroll()
                        .id(id)
                        .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                        .show(ui, |ui| {
                            fixed_block(ui, WidgetId::from_hash("content"), w, h_content);
                        });
                });
        };
        let mut h = UiHarness::new(SURFACE);
        h.frame(build);
        h.move_to(Vec2::new(50.0, 50.0));
        assert_eq!(h.ui.input().scroll_targets(), first, "{label}: first frame");
        h.frame(build);
        assert_eq!(h.ui.input().scroll_targets(), settled, "{label}: settled");
    }
}

/// [`Scroll::reveal`] pans by the least distance that shows a row: ten 20 px rows in a 50 px
/// viewport. Row 5 (100..120) below the view lands at its bottom, offset 120 - 50 = 70; row 4
/// (80..100) is then in view and moves nothing; row 1 (20..40) above it lands at its top,
/// offset 20. An 80 px row at 30, taller than the view, lands with its start at the top.
#[test]
fn reveal_pans_the_least_that_shows_a_child() {
    let row = |idx: usize| WidgetId::from_hash(("row", idx));
    let scene = |ui: &mut Ui, heights: &[f32], reveal: Option<usize>| {
        let mut scroll = Scroll::vertical()
            .id(WidgetId::from_hash("scroll"))
            .size((Sizing::fixed(200.0), Sizing::fixed(50.0)));
        if let Some(idx) = reveal {
            scroll = scroll.reveal(row(idx));
        }
        scroll.show(ui, |ui| {
            Panel::vstack().id_salt("rows").show(ui, |ui| {
                for (idx, h) in heights.iter().enumerate() {
                    fixed_block(ui, row(idx), 200.0, *h);
                }
            });
        });
    };
    let rows = [20.0; 10];
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| scene(ui, &rows, None));
    for (reveal, offset, why) in [
        (5, 70.0, "a row below lands at the bottom"),
        (4, 70.0, "a row in view moves nothing"),
        (1, 20.0, "a row above lands at the top"),
    ] {
        h.frame(|ui| scene(ui, &rows, Some(reveal)));
        assert_eq!(read_state(&h).offset().y, offset, "row {reveal}: {why}");
    }

    let tall = [30.0, 80.0, 30.0];
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| scene(ui, &tall, None));
    h.frame(|ui| scene(ui, &tall, Some(1)));
    assert_eq!(
        read_state(&h).offset().y,
        30.0,
        "a row taller than the view shows its start"
    );
}
