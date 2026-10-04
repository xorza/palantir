//! What a pinch, a wheel and a modifier do to the scale.

use crate::TextStyle;
use crate::Ui;
use crate::internals::harness::UiHarness;
use crate::internals::panic_probe;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use crate::widgets::panel::Panel;
use crate::widgets::scroll::Scroll;
use crate::widgets::scroll::state::ScrollState;
use crate::widgets::scroll::tests::support::{
    SURFACE, build, fixed_block, read_state, zoom_driven,
};
use crate::widgets::scroll::zoom_config::ZoomConfig;
use glam::{UVec2, Vec2};

#[test]
fn nested_non_zoom_scroll_routes_pinch_to_zoomable_ancestor() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    let outer_id = WidgetId::from_hash("outer");
    let inner_id = WidgetId::from_hash("inner");
    let build = |ui: &mut Ui| {
        Scroll::both()
            .id(outer_id)
            .zoomable()
            .size((Sizing::fixed(300.0), Sizing::fixed(300.0)))
            .show(ui, |ui| {
                Scroll::vertical()
                    .id(inner_id)
                    .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                    .show(ui, |ui| {
                        fixed_block(ui, WidgetId::from_hash("content"), 400.0, 400.0);
                    });
            });
    };
    h.frame(build);

    h.move_to(Vec2::new(50.0, 50.0));
    assert_eq!(h.ui.input().scroll_targets.y, Some(inner_id));
    assert_eq!(h.ui.input().pinch_target, Some(outer_id));
    assert!(h.pinch(1.5).repaint_requested);
    h.frame(build);

    let outer_zoom = h.state::<ScrollState>(outer_id).zoom;
    let inner_zoom = h.state::<ScrollState>(inner_id).zoom;
    assert_eq!(outer_zoom, 1.5);
    assert_eq!(inner_zoom, 1.0);
}

#[test]
fn pinch_zoom_keeps_point_under_cursor_fixed() {
    const OUTER_PAD: f32 = 16.0;
    const TEXT_GAP: f32 = 24.0;

    #[derive(Debug)]
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
                    Block::new()
                        .id(WidgetId::from_hash("topbar"))
                        .size((Sizing::fixed(200.0), Sizing::fixed(TEXT_GAP)))
                        .show(ui);
                    Scroll::both()
                        .id(WidgetId::from_hash("xy"))
                        .zoomable()
                        .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                        .show(ui, |ui| {
                            fixed_block(
                                ui,
                                WidgetId::from_hash("content"),
                                content_size,
                                content_size,
                            );
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
        let before = *h.state::<ScrollState>(id);
        let pivot_local = Vec2::new(pointer.0 - OUTER_PAD, pointer.1 - (OUTER_PAD + TEXT_GAP));
        let world_before = Vec2::new(
            (pivot_local.x + before.offset.x) / before.zoom,
            (pivot_local.y + before.offset.y) / before.zoom,
        );

        for &pinch in pinches {
            h.pinch(pinch);
            h.frame(build);
        }

        let after = *h.state::<ScrollState>(id);
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
                    .zoomable()
                    .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                    .show(ui, |ui| {
                        fixed_block(ui, WidgetId::from_hash("content"), 400.0, 400.0);
                    });
            });
    };
    h.frame(build);

    let id = WidgetId::from_hash("xy");
    {
        let row = h.ui.state_or_default::<ScrollState>(id);
        row.offset = Vec2::new(0.0, -50.0);
    }

    h.scroll_pixels_at(Vec2::new(50.0, 50.0), Vec2::new(0.0, 5.0));
    h.frame(build);

    let after = *h.state::<ScrollState>(id);
    assert_eq!(
        after.offset.y, -45.0,
        "wheel pan from out-of-range offset snapped: -50 + 5 should be -45, got {}",
        after.offset.y
    );

    h.scroll_pixels(Vec2::new(0.0, -5.0));
    h.frame(build);
    let after2 = *h.state::<ScrollState>(id);
    assert_eq!(
        after2.offset.y, -45.0,
        "pan further out-of-range should be blocked at current ({}), got {}",
        -45.0, after2.offset.y
    );
}

#[test]
fn pivot_zoom_preserves_underflow_pan_range() {
    let mut h = UiHarness::new(SURFACE);
    let build = |ui: &mut Ui| {
        Scroll::both()
            .id(WidgetId::from_hash("scroll"))
            .zoomable()
            .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
            .show(ui, |ui| {
                fixed_block(ui, WidgetId::from_hash("content"), 100.0, 100.0);
            });
    };
    h.frame(build);
    h.pinch_at(Vec2::new(50.0, 50.0), 0.5);
    h.frame(build);

    let id = WidgetId::from_hash("scroll");
    let zoomed = *h.state::<ScrollState>(id);
    let expected_zoomed_offset = f32::midpoint(0.0, 50.0) - 50.0;
    assert_eq!(zoomed.zoom, 0.5);
    assert_eq!(zoomed.offset.y, expected_zoomed_offset);

    h.scroll_pixels(Vec2::new(0.0, -10.0));
    h.frame(build);
    let panned = *h.state::<ScrollState>(id);
    assert_eq!(panned.offset.y, expected_zoomed_offset - 10.0);
    assert_ne!(panned.offset.y, zoomed.offset.y);
}

#[test]
fn ctrl_touchpad_pixel_scroll_zooms_at_same_rate_as_wheel_lines() {
    use crate::input::keyboard::modifiers::Modifiers;

    // The wheel-step refactor split lines vs pixels at the input
    // layer; the zoom path must combine them so a touchpad gesture
    // under ctrl still zooms — pre-split it did, and regressing that
    // breaks touchpad pinch-via-modifier. Two lines' worth of touchpad
    // pixels is two virtual notches.
    let mut h = UiHarness::new(SURFACE);
    let build_zoom = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Scroll::both()
                    .id(WidgetId::from_hash("zoomy"))
                    .zoomable()
                    .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                    .show(ui, |ui| {
                        fixed_block(ui, WidgetId::from_hash("content"), 800.0, 800.0);
                    });
            });
    };
    h.frame(build_zoom);

    let scroll_id = WidgetId::from_hash("zoomy");
    let before_zoom = h.state::<ScrollState>(scroll_id).zoom;

    // Press ctrl, then touchpad-scroll. `wheel_zoom_gate` requires
    // ctrl||cmd; with cfg.step = 1.03 the factor is 1.03^(-2) ≈ 0.9426.
    h.move_onto(scroll_id);
    h.set_modifiers(Modifiers::CTRL);
    let line_px = TextStyle::default().line_height_for(16.0);
    h.scroll_pixels(Vec2::new(0.0, 2.0 * line_px));
    h.frame(build_zoom);

    let after_zoom = h.state::<ScrollState>(scroll_id).zoom;
    let expected = before_zoom * 1.03_f32.powf(-2.0);
    assert_eq!(
        after_zoom, expected,
        "ctrl+touchpad zoom: expected {expected}, got {after_zoom}"
    );
}

#[test]
fn wheel_zoom_step_is_font_independent() {
    // One wheel line = one zoom notch, regardless of theme font size.
    // The line→pan magnitude scales with font; the line→zoom step must
    // not — pin that so a future refactor that reintroduces a
    // font-scaled denominator on the zoom side fails loudly.
    // `ZoomConfig::default().step` is 1.03, and scrolling down zooms out.
    let expected = 1.03_f32.powf(-1.0);
    for font_size in [12.0_f32, 16.0, 24.0] {
        use crate::input::keyboard::modifiers::Modifiers;

        let mut h = UiHarness::new(SURFACE);
        h.ui.theme_mut().text.font_size = font_size;
        let build_zoom = |ui: &mut Ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |ui| {
                    Scroll::both()
                        .id(WidgetId::from_hash("fz"))
                        .zoomable()
                        .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                        .show(ui, |ui| {
                            fixed_block(ui, WidgetId::from_hash("content"), 800.0, 800.0);
                        });
                });
        };
        h.frame(build_zoom);

        h.move_onto(WidgetId::from_hash("fz"));
        h.set_modifiers(Modifiers::CTRL);
        h.scroll_lines(Vec2::new(0.0, 1.0));
        h.frame(build_zoom);

        let scroll_id = WidgetId::from_hash("fz");
        let zoom = h.state::<ScrollState>(scroll_id).zoom;
        // A tolerance for `powf`'s rounding only: a font-scaled step would
        // miss by the font ratio, two orders of magnitude more.
        assert_eq!(
            zoom, expected,
            "one wheel line is one zoom step at font_size {font_size}: expected {expected}, got {zoom}"
        );
    }
}

#[test]
fn line_wheel_step_scales_with_theme_font_size() {
    // Pin: a `ScrollLines(0, 1)` event lands one laid-out line of pan —
    // `font_size * line_height_factor` on the shaper's 1/64-px grid — not
    // the legacy 40 px constant. 16 × 1.2 = 19.2 is 1228.8 64ths, which
    // rounds to 1229: 19.203125. 24 × 1.5 = 36 is on the grid.
    let cases: &[(&str, f32, f32, f32)] = &[
        ("default_16px_text", 16.0, 1.2, 1229.0 / 64.0),
        ("larger_24px_text", 24.0, 1.5, 36.0),
    ];
    for (label, font_size, line_height_factor, expected_px) in cases {
        let mut h = UiHarness::new(SURFACE);
        let text = &mut h.ui.theme_mut().text;
        text.font_size = *font_size;
        text.line_height_factor = *line_height_factor;
        let build_v = |ui: &mut Ui| build(ui, 200.0, 800.0);
        h.frame(build_v);
        h.scroll_lines_at(Vec2::new(50.0, 50.0), Vec2::new(0.0, 1.0));
        h.frame(build_v);

        let scroll_id = WidgetId::from_hash("scroll");
        let offset_y = h.state::<ScrollState>(scroll_id).offset.y;
        assert_eq!(
            offset_y, *expected_px,
            "case: {label} — expected {expected_px} px after 1 line wheel, got {offset_y}"
        );
    }
}

/// [`Scroll::zoom_by`] scales through the same clamp a pinch takes.
///
/// The default [`ZoomConfig`] range is what stops it, so the case walks
/// past both ends and asks where it stopped rather than pinning the
/// range itself.
#[test]
fn zoom_by_scales_and_clamps_into_the_configured_range() {
    let cfg = ZoomConfig::default();
    let (min_zoom, max_zoom) = (*cfg.range.start(), *cfg.range.end());
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| zoom_driven(ui, &[1.0]));
    assert_eq!(
        read_state(&mut h).zoom,
        1.0,
        "premise: an identity request is a no-op",
    );

    h.frame(|ui| zoom_driven(ui, &[1.5]));
    assert_eq!(
        read_state(&mut h).zoom,
        1.5,
        "the factor multiplies the zoom"
    );

    h.frame(|ui| zoom_driven(ui, &[2.0]));
    assert_eq!(
        read_state(&mut h).zoom,
        3.0,
        "and multiplies again, from where the last frame left it",
    );

    for _ in 0..40 {
        h.frame(|ui| zoom_driven(ui, &[2.0]));
    }
    assert_eq!(read_state(&mut h).zoom, max_zoom, "clamped at the top");

    for _ in 0..80 {
        h.frame(|ui| zoom_driven(ui, &[0.5]));
    }
    assert_eq!(read_state(&mut h).zoom, min_zoom, "and at the bottom");
}

/// Two calls on one builder compose into one factor, so a caller may
/// fold a request in from more than one place.
#[test]
fn zoom_by_composes_across_calls() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| zoom_driven(ui, &[1.0]));
    h.frame(|ui| zoom_driven(ui, &[1.5, 2.0]));
    assert_eq!(
        read_state(&mut h).zoom,
        3.0,
        "1.5 then 2.0 is one factor of 3",
    );
}

/// A zoom factor is authored, not data, so an impossible one is a caller
/// error rather than a silently ignored request.
#[test]
fn zoom_by_rejects_a_factor_that_cannot_scale() {
    for bad in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        panic_probe::assert_panics_with("a positive value must be finite and above zero", || {
            let _ = Scroll::both().zoom_by(bad);
        });
    }
    // A single-axis scroll has no zoom: it panics where the zoom is asked
    // for, in every build, through either setter.
    for scroll in [Scroll::vertical, Scroll::horizontal] {
        panic_probe::assert_panics_with("a zoomable scroll must pan on both axes", || {
            let _ = scroll().zoomable();
        });
        panic_probe::assert_panics_with("a zoomable scroll must pan on both axes", || {
            let _ = scroll().zoom_config(ZoomConfig::default());
        });
    }
}

/// The offset band reaches both ends of zoomed content inside padding.
///
/// 200 × 200 viewport, padding 10, content 400 × 400, zoom 2. Each axis
/// shows `200 - gutter - 2 × 10` px, and the content spans `400 × 2 = 800`.
/// Panned to either end, the content's own edge sits exactly on the
/// viewport's — the start on the padding's inner edge at offset 0, the end
/// on the far edge at offset `800 - shown`. Scaled about the node's corner
/// instead, the padding grew to 20 and both ends missed by 10.
#[test]
fn zoomed_padding_keeps_both_content_ends_reachable() {
    let scroll_id = WidgetId::from_hash("scroll");
    let content_id = WidgetId::from_hash("content");
    let show = |ui: &mut Ui, zoom: f32, pan: Vec2| {
        Scroll::both()
            .id(scroll_id)
            .zoomable()
            .zoom_by(zoom)
            .pan_by(pan)
            .padding(10.0)
            .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
            .show(ui, |ui| fixed_block(ui, content_id, 400.0, 400.0));
    };
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| show(ui, 1.0, Vec2::ZERO));
    h.frame(|ui| show(ui, 2.0, Vec2::ZERO));
    let gutter = 200.0 - 2.0 * 10.0 - h.rect(content_id).unwrap().size.w;
    let shown = 200.0 - gutter - 2.0 * 10.0;

    for (pan, offset, edge) in [
        (Vec2::splat(-1e4), 0.0, 10.0),
        (Vec2::splat(1e4), 800.0 - shown, 10.0 + shown),
    ] {
        h.frame(|ui| show(ui, 1.0, pan));
        h.frame(|ui| show(ui, 1.0, Vec2::ZERO));
        assert_eq!(
            h.state::<ScrollState>(scroll_id).offset,
            Vec2::splat(offset)
        );
        let laid = h.arranged(content_id);
        let transform = h.transform(content_id);
        let start = transform.apply_point(laid.min);
        let end = transform.apply_point(laid.max());
        let reached = if offset == 0.0 { start } else { end };
        assert_eq!(reached, Vec2::splat(edge), "panned by {pan:?}");
    }
}

/// `zoom_config` carries its range to the zoom: one 0.25× pinch lands
/// at 0.25 under the default 0.1..=10 range, and clamps to the floor of a
/// 0.5..=2 one.
#[test]
fn zoom_config_clamps_to_its_own_range() {
    let id = WidgetId::from_hash("ranged");
    for (config, want) in [
        (ZoomConfig::default(), 0.25),
        (ZoomConfig::new(0.5..=2.0, 1.25), 0.5),
    ] {
        let mut h = UiHarness::new(SURFACE);
        let build = |ui: &mut Ui| {
            Scroll::both()
                .id(id)
                .zoom_config(config.clone())
                .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                .show(ui, |ui| {
                    fixed_block(ui, WidgetId::from_hash("ranged-content"), 100.0, 100.0);
                });
        };
        h.frame(build);
        h.pinch_at(Vec2::new(50.0, 50.0), 0.25);
        h.frame(build);
        assert_eq!(h.state::<ScrollState>(id).zoom, want);
    }
}

/// The modifier decides which wheel zooms. One line down with and without
/// Ctrl, under each setting: `Ctrl` zooms only the Ctrl wheel, `Always`
/// zooms both, and `PinchOnly` neither. One line is one step, `1.03^-1`.
#[test]
fn zoom_modifier_picks_which_wheel_zooms() {
    use crate::input::keyboard::modifiers::Modifiers;
    use crate::widgets::scroll::zoom_config::ZoomModifier;

    let id = WidgetId::from_hash("modded");
    let step = 1.03_f32.powf(-1.0);
    for (modifier, bare, ctrl) in [
        (ZoomModifier::Ctrl, 1.0, step),
        (ZoomModifier::Always, step, step),
        (ZoomModifier::PinchOnly, 1.0, 1.0),
    ] {
        let ctrl_held = Modifiers::CTRL;
        for (held, want) in [(Modifiers::NONE, bare), (ctrl_held, ctrl)] {
            let mut h = UiHarness::new(SURFACE);
            let config = ZoomConfig::default().with_modifier(modifier);
            let build = |ui: &mut Ui| {
                Scroll::both()
                    .id(id)
                    .zoom_config(config.clone())
                    .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                    .show(ui, |ui| {
                        fixed_block(ui, WidgetId::from_hash("modded-content"), 800.0, 800.0);
                    });
            };
            h.frame(build);
            h.move_onto(id);
            h.set_modifiers(held);
            h.scroll_lines(Vec2::new(0.0, 1.0));
            h.frame(build);
            assert_eq!(
                h.state::<ScrollState>(id).zoom,
                want,
                "{modifier:?} with {held:?}",
            );
        }
    }
}

/// The pivot is the point a zoom step holds still. A 2× pinch at (50, 50)
/// over a 200 × 200 viewport at the origin: under `Pointer` content point
/// (50, 50) stays under the pointer, so it moves to (100, 100) and the
/// offset becomes 100 − 50 = 50; under `Center` the viewport centre
/// (100, 100) stays, so it moves to (200, 200) and the offset becomes
/// 200 − 100 = 100.
#[test]
fn zoom_pivot_picks_the_point_a_step_holds() {
    use crate::widgets::scroll::zoom_config::ZoomPivot;

    let id = WidgetId::from_hash("pivoted");
    for (pivot, want) in [(ZoomPivot::Pointer, 50.0), (ZoomPivot::Center, 100.0)] {
        let mut h = UiHarness::new(SURFACE);
        let config = ZoomConfig::default().with_pivot(pivot);
        let build = |ui: &mut Ui| {
            Scroll::both()
                .id(id)
                .zoom_config(config.clone())
                .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                .show(ui, |ui| {
                    fixed_block(ui, WidgetId::from_hash("pivoted-content"), 800.0, 800.0);
                });
        };
        h.frame(build);
        h.pinch_at(Vec2::new(50.0, 50.0), 2.0);
        h.frame(build);
        let state = h.state::<ScrollState>(id);
        assert_eq!(state.zoom, 2.0, "{pivot:?}");
        assert_eq!(state.offset, Vec2::splat(want), "{pivot:?}");
    }
}
