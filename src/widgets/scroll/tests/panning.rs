//! What moves the offset, and where it is clamped.

use crate::Ui;
use crate::layout::types::sizing::Sizing;
use crate::primitives::size::Size;
use crate::primitives::translate_scale::TranslateScale;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::Configure;
use crate::ui::harness::UiHarness;
use crate::widgets::frame::Frame;
use crate::widgets::panel::Panel;
use crate::widgets::scroll::Scroll;
use crate::widgets::scroll::state::ScrollState;
use crate::widgets::scroll::tests::support::{
    SURFACE, build, read_state, scroll_content, scroll_viewport,
};
use glam::Vec2;

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
        h.move_to(Vec2::new(50.0, 50.0));
        for wheel_y in *pushes {
            h.scroll_pixels(Vec2::new(0.0, *wheel_y));
            h.frame(|ui| build(ui, *viewport_h, *content_h));
        }

        assert_eq!(read_state(&mut h).offset.y, *expected, "case: {label}");
    }
}

/// `content_margin` shifts the natural offset range away from
/// `[0, slack]`: a left/top margin of `m` opens a `[-m, 0)` band so
/// the user can pan past the children's origin; the right/bottom
/// margin extends the upper bound by the same amount. Symmetric
/// margin = symmetric range about the natural `[0, slack]`.
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
                Frame::new()
                    .id(WidgetId::from_hash("content"))
                    .size((Sizing::fixed(400.0), Sizing::fixed(400.0)))
                    .show(ui);
            });
    };
    h.frame(build_m);
    h.move_to(Vec2::new(50.0, 50.0));
    // Pan left/up: large negative wheel delta should clamp at `-m` on
    // both axes (margin is symmetric and zoom is 1.0).
    h.scroll_pixels(Vec2::new(-9_999.0, -9_999.0));
    h.frame(build_m);
    assert_eq!(read_state(&mut h).offset, Vec2::new(-m, -m));
    // Pan back the other way: clamp at `raw_slack + m`. Raw slack =
    // 400 - 200 = 200; total max = 200 + 100 = 300.
    h.scroll_pixels(Vec2::new(9_999.0, 9_999.0));
    h.frame(build_m);
    let raw_slack = 400.0 - 200.0;
    assert_eq!(
        read_state(&mut h).offset,
        Vec2::new(raw_slack + m, raw_slack + m)
    );
}

#[test]
fn horizontal_scroll_pans_only_x() {
    let mut h = UiHarness::new(SURFACE);
    let build_h = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Scroll::horizontal()
                    .id(WidgetId::from_hash("hscroll"))
                    .size((Sizing::fixed(200.0), Sizing::fixed(40.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .id(WidgetId::from_hash("hcontent"))
                            .size((Sizing::fixed(800.0), Sizing::fixed(40.0)))
                            .show(ui);
                    });
            });
    };
    h.frame(build_h);
    h.scroll_pixels_at(Vec2::new(50.0, 20.0), Vec2::new(75.0, 200.0));

    h.frame(build_h);
    let id = WidgetId::from_hash("hscroll");
    let row = *h.ui.state_mut::<ScrollState>(id);
    assert_eq!(row.offset, Vec2::new(75.0, 0.0));
}

#[test]
fn both_axis_scroll_pans_both_axes() {
    let mut h = UiHarness::new(SURFACE);
    let build_xy = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Scroll::both()
                    .id(WidgetId::from_hash("xy"))
                    .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .id(WidgetId::from_hash("xy-content"))
                            .size((Sizing::fixed(800.0), Sizing::fixed(800.0)))
                            .show(ui);
                    });
            });
    };
    h.frame(build_xy);
    h.scroll_pixels_at(Vec2::new(50.0, 50.0), Vec2::new(40.0, 60.0));

    h.frame(build_xy);
    let id = WidgetId::from_hash("xy");
    let row = *h.ui.state_mut::<ScrollState>(id);
    assert_eq!(row.offset, Vec2::new(40.0, 60.0));
    assert_eq!(scroll_content(&h.ui, id), Size::new(800.0, 800.0));
    // Viewport reserves `theme.width + theme.gap = 12px` per panned
    // axis when content overflows; 200 - 12 = 188.
    assert_eq!(scroll_viewport(&h.ui, id), Size::new(188.0, 188.0));
}

/// Press on the V thumb, drag down; `ScrollState.offset.y` moves
/// `delta * (content - viewport) / (track - thumb)` clamped to
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
                            Frame::new()
                                .id(WidgetId::from_hash("tall"))
                                .size((Sizing::fixed(180.0), Sizing::fixed(800.0)))
                                .show(ui);
                        });
                });
        };
        h.frame(build);
        h.frame(build);

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
        let offset_y = h.ui.state_mut::<ScrollState>(outer_id).offset.y;
        assert!(
            (offset_y - 120.0).abs() < 0.5,
            "30 logical px at {scale}× should produce offset 120, got {offset_y}",
        );

        h.move_to(press + Vec2::new(0.0, 9_999.0 * scale));
        h.frame(build);
        assert_eq!(
            h.ui.state_mut::<ScrollState>(outer_id).offset.y,
            600.0,
            "drag past end at {scale}× clamps to max offset",
        );
    }
}

#[test]
fn click_on_track_before_thumb_pages_back_after_pages_forward() {
    // Both axes follow the same code path; pin both so the symmetric
    // helper can't drift. For each axis: click far end of track →
    // page forward by one viewport; click near end → page back to 0.
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
                                            Frame::new()
                                                .id(WidgetId::from_hash("hcontent"))
                                                .size((Sizing::fixed(800.0), Sizing::fixed(40.0)))
                                                .show(ui);
                                        });
                                });
                        }
                    });
            };
            h.frame(build_axis);

            let outer_id = WidgetId::from_hash(*scroll_key);
            let scroll_id = outer_id.with("viewport");
            let track_id = scroll_id.with(*track_suffix);
            let track = h.ui.response_for(track_id);
            let layout = track.layout_rect.expect("track arranged");
            let (forward_local, back_local) = match axis {
                AxisCase::V => (Vec2::new(6.0, 196.0), Vec2::new(6.0, 4.0)),
                AxisCase::H => (Vec2::new(196.0, 6.0), Vec2::new(4.0, 6.0)),
            };
            let forward_press = track.transform.apply_point(layout.min + forward_local);
            let back_press = track.transform.apply_point(layout.min + back_local);

            h.press_at(forward_press);
            h.release();
            h.frame(build_axis);
            let offset = h.ui.state_mut::<ScrollState>(outer_id).offset;
            let forward = match axis {
                AxisCase::V => offset.y,
                AxisCase::H => offset.x,
            };
            assert_eq!(
                forward, *page_step,
                "case: {label} at {scale}× — click past thumb pages forward",
            );

            h.press_at(back_press);
            h.release();
            h.frame(build_axis);
            let offset = h.ui.state_mut::<ScrollState>(outer_id).offset;
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

/// Regression: a non-zoomable scroll must pull its offset back into
/// range when the content *shrinks* with no wheel/drag input —
/// otherwise the viewport stays stranded in the now-empty tail. The
/// record-time clamp reads the previous frame's arranged content, so
/// the correction lands the frame after the shrink's arrange (hence the
/// extra settle frame); the bug was that the clamp only ran on nonzero
/// pan input, so a passive shrink never triggered it.
#[test]
fn shrinking_content_unstrands_offset_without_input() {
    let mut h = UiHarness::new(SURFACE);
    // Scroll an 800px content to the bottom of a 200px viewport.
    h.frame(|ui| build(ui, 200.0, 800.0));
    h.scroll_pixels_at(Vec2::new(50.0, 50.0), Vec2::new(0.0, 10_000.0));
    h.frame(|ui| build(ui, 200.0, 800.0));
    assert_eq!(
        read_state(&mut h).offset.y,
        600.0,
        "precondition: scrolled to max (800 - 200)",
    );

    // Content shrinks to 300px (new max = 100), NO input. Frame 1
    // records against the stale 800px content (offset stays 600) and
    // arranges the new 300px content; frame 2 records against the fresh
    // 300px content and clamps the stranded offset down.
    h.frame(|ui| build(ui, 200.0, 300.0));
    h.frame(|ui| build(ui, 200.0, 300.0));
    assert_eq!(
        read_state(&mut h).offset.y,
        100.0,
        "offset must clamp to the new max (300 - 200) after a passive content shrink",
    );
}

/// Content that fits its viewport rests at offset zero even when a
/// leading `content_margin` opens a band below it.
///
/// `content_margin` is documented as invisible overscroll that leaves
/// child layout alone, so it may only widen the pannable range — never
/// move where content sits at rest. The old `natural_bounds` folded the
/// trailing margin in before flooring the overflow, so once content fit
/// the trailing endpoint fell *below* the leading one, `hi.max(lo)`
/// collapsed the band to the single value `-m`, and the settle clamp
/// pinned the content there: a 100 px margin shoved a fitting child
/// 100 px sideways and left it stuck.
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
                // Smaller than the 200x200 viewport, so there is no
                // overflow to pan and the resting offset must be zero.
                Frame::new()
                    .id(WidgetId::from_hash("content"))
                    .size((Sizing::fixed(80.0), Sizing::fixed(80.0)))
                    .show(ui);
            });
    };
    h.frame(build);
    h.frame(build);
    assert_eq!(
        read_state(&mut h).offset,
        Vec2::ZERO,
        "fitting content must rest at the origin, not at the leading margin",
    );

    // The margin still opens its band — the user can pan into it and
    // back out to rest. This is what distinguishes the fix from simply
    // dropping the margin when content fits.
    h.move_to(Vec2::new(50.0, 50.0));
    h.scroll_pixels(Vec2::new(-9_999.0, -9_999.0));
    h.frame(build);
    assert_eq!(
        read_state(&mut h).offset,
        Vec2::new(-m, -m),
        "the leading band is still reachable for content that fits",
    );
    h.scroll_pixels(Vec2::new(9_999.0, 9_999.0));
    h.frame(build);
    assert_eq!(
        read_state(&mut h).offset,
        Vec2::new(m, m),
        "and so is the trailing band",
    );
}
