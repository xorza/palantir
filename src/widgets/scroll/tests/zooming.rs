//! What a pinch, a wheel and a modifier do to the scale.

use crate::Ui;
use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::Configure;
use crate::ui::harness::UiHarness;
use crate::widgets::frame::Frame;
use crate::widgets::panel::Panel;
use crate::widgets::scroll::Scroll;
use crate::widgets::scroll::state::ScrollState;
use crate::widgets::scroll::tests::support::{SURFACE, build};
use glam::{UVec2, Vec2};

#[test]
fn nested_non_zoom_scroll_routes_pinch_to_zoomable_ancestor() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    let outer_id = WidgetId::from_hash("outer");
    let inner_id = WidgetId::from_hash("inner");
    let build = |ui: &mut Ui| {
        Scroll::both()
            .id(outer_id)
            .with_zoom()
            .size((Sizing::fixed(300.0), Sizing::fixed(300.0)))
            .show(ui, |ui| {
                Scroll::vertical()
                    .id(inner_id)
                    .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .id(WidgetId::from_hash("content"))
                            .size((Sizing::fixed(400.0), Sizing::fixed(400.0)))
                            .show(ui);
                    });
            });
    };
    h.frame(build);

    h.move_to(Vec2::new(50.0, 50.0));
    assert_eq!(h.ui.input().scroll_target, Some(inner_id));
    assert_eq!(h.ui.input().pinch_target, Some(outer_id));
    assert!(h.pinch(1.5).requests_repaint);
    h.frame(build);

    let outer_zoom = h.ui.state_mut::<ScrollState>(outer_id).zoom;
    let inner_zoom = h.ui.state_mut::<ScrollState>(inner_id).zoom;
    assert_eq!(outer_zoom, 1.5);
    assert_eq!(inner_zoom, 1.0);
}

#[test]
fn pinch_zoom_keeps_point_under_cursor_fixed() {
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
        let mut h = UiHarness::new(SURFACE);
        let build = |ui: &mut Ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .padding(OUTER_PAD)
                .show(ui, |ui| {
                    Frame::new()
                        .id(WidgetId::from_hash("topbar"))
                        .size((Sizing::fixed(200.0), Sizing::fixed(TEXT_GAP)))
                        .show(ui);
                    Scroll::both()
                        .id(WidgetId::from_hash("xy"))
                        .with_zoom()
                        .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                        .show(ui, |ui| {
                            Frame::new()
                                .id(WidgetId::from_hash("content"))
                                .size((Sizing::fixed(content_size), Sizing::fixed(content_size)))
                                .show(ui);
                        });
                });
        };

        h.frame(build);

        h.move_to(Vec2::new(pointer.0, pointer.1));
        for &(px, py) in pans {
            h.scroll_pixels(Vec2::new(px, py));
            h.frame(build);
        }

        let id = WidgetId::from_hash("xy");
        let before = *h.ui.state_mut::<ScrollState>(id);
        let pivot_local = Vec2::new(pointer.0 - OUTER_PAD, pointer.1 - (OUTER_PAD + TEXT_GAP));
        let world_before = Vec2::new(
            (pivot_local.x + before.offset.x) / before.zoom,
            (pivot_local.y + before.offset.y) / before.zoom,
        );

        for &pinch in pinches {
            h.pinch(pinch);
            h.frame(build);
        }

        let after = *h.ui.state_mut::<ScrollState>(id);
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
    let mut h = UiHarness::new(SURFACE);
    let build = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Scroll::both()
                    .id(WidgetId::from_hash("xy"))
                    .with_zoom()
                    .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .id(WidgetId::from_hash("content"))
                            .size((Sizing::fixed(400.0), Sizing::fixed(400.0)))
                            .show(ui);
                    });
            });
    };
    h.frame(build);

    let id = WidgetId::from_hash("xy");
    {
        let row = h.ui.state_mut::<ScrollState>(id);
        row.offset = Vec2::new(0.0, -50.0);
    }

    h.scroll_pixels_at(Vec2::new(50.0, 50.0), Vec2::new(0.0, 5.0));
    h.frame(build);

    let after = *h.ui.state_mut::<ScrollState>(id);
    assert!(
        (after.offset.y - (-45.0)).abs() < 1e-3,
        "wheel pan from out-of-range offset snapped: -50 + 5 should be -45, got {}",
        after.offset.y,
    );

    h.scroll_pixels(Vec2::new(0.0, -5.0));
    h.frame(build);
    let after2 = *h.ui.state_mut::<ScrollState>(id);
    assert!(
        (after2.offset.y - (-45.0)).abs() < 1e-3,
        "pan further out-of-range should be blocked at current ({}), got {}",
        -45.0,
        after2.offset.y,
    );
}

#[test]
fn pivot_zoom_preserves_underflow_pan_range() {
    let mut h = UiHarness::new(SURFACE);
    let build = |ui: &mut Ui| {
        Scroll::both()
            .id(WidgetId::from_hash("scroll"))
            .with_zoom()
            .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("content"))
                    .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
                    .show(ui);
            });
    };
    h.frame(build);
    h.pinch_at(Vec2::new(50.0, 50.0), 0.5);
    h.frame(build);

    let id = WidgetId::from_hash("scroll");
    let zoomed = *h.ui.state_mut::<ScrollState>(id);
    let expected_zoomed_offset = (0.0 + 50.0) * 0.5 - 50.0;
    assert_eq!(zoomed.zoom, 0.5);
    assert_eq!(zoomed.offset.y, expected_zoomed_offset);

    h.scroll_pixels(Vec2::new(0.0, -10.0));
    h.frame(build);
    let panned = *h.ui.state_mut::<ScrollState>(id);
    assert_eq!(panned.offset.y, expected_zoomed_offset - 10.0);
    assert_ne!(panned.offset.y, zoomed.offset.y);
}

#[test]
fn ctrl_touchpad_pixel_scroll_zooms_at_same_rate_as_wheel_lines() {
    // The wheel-step refactor split lines vs pixels at the input
    // layer; the zoom path must combine them so a touchpad gesture
    // under ctrl still zooms — pre-split it did, and regressing that
    // breaks touchpad pinch-via-modifier. With line_px = 19.2 (default
    // 16 × 1.2), 38.4 px of touchpad scroll = 2 virtual notches.
    let mut h = UiHarness::new(SURFACE);
    let build_zoom = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Scroll::both()
                    .id(WidgetId::from_hash("zoomy"))
                    .with_zoom()
                    .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .id(WidgetId::from_hash("content"))
                            .size((Sizing::fixed(800.0), Sizing::fixed(800.0)))
                            .show(ui);
                    });
            });
    };
    h.frame(build_zoom);

    let scroll_id = WidgetId::from_hash("zoomy");
    let before_zoom = h.ui.state_mut::<ScrollState>(scroll_id).zoom;

    // Press ctrl, then touchpad-scroll. `wheel_zoom_gate` requires
    // ctrl||cmd; with cfg.step = 1.03 the factor is 1.03^(-2) ≈ 0.9426.
    use crate::input::keyboard::Modifiers;
    h.move_onto(scroll_id);
    h.set_modifiers(Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    });
    h.scroll_pixels(Vec2::new(0.0, 38.4));
    h.frame(build_zoom);

    let after_zoom = h.ui.state_mut::<ScrollState>(scroll_id).zoom;
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
        let mut h = UiHarness::new(SURFACE);
        h.ui.theme_mut().text.font_size_px = font_size;
        let build_zoom = |ui: &mut Ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |ui| {
                    Scroll::both()
                        .id(WidgetId::from_hash("fz"))
                        .with_zoom()
                        .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                        .show(ui, |ui| {
                            Frame::new()
                                .id(WidgetId::from_hash("content"))
                                .size((Sizing::fixed(800.0), Sizing::fixed(800.0)))
                                .show(ui);
                        });
                });
        };
        h.frame(build_zoom);

        use crate::input::keyboard::Modifiers;
        h.move_onto(WidgetId::from_hash("fz"));
        h.set_modifiers(Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        });
        h.scroll_lines(Vec2::new(0.0, 1.0));
        h.frame(build_zoom);

        let scroll_id = WidgetId::from_hash("fz");
        let zoom = h.ui.state_mut::<ScrollState>(scroll_id).zoom;
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
        let mut h = UiHarness::new(SURFACE);
        let text = &mut h.ui.theme_mut().text;
        text.font_size_px = *font_size;
        text.line_height_mult = *line_height_mult;
        let build_v = |ui: &mut Ui| build(ui, 200.0, 800.0);
        h.frame(build_v);
        h.scroll_lines_at(Vec2::new(50.0, 50.0), Vec2::new(0.0, 1.0));
        h.frame(build_v);

        let scroll_id = WidgetId::from_hash("scroll");
        let offset_y = h.ui.state_mut::<ScrollState>(scroll_id).offset.y;
        assert!(
            (offset_y - expected_px).abs() < 0.01,
            "case: {label} — expected {expected_px} px after 1 line wheel, got {offset_y}",
        );
    }
}
