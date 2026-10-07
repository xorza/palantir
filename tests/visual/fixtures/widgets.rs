//! Per-widget fixtures: the smallest scene exercising one widget's render path.

use glam::{UVec2, Vec2};
use palantir::golden::image::Rgba;
use palantir::widget::Shape;
use palantir::{
    Background, Block, Button, ColorCoords, ColorField, ColorModel, ColorPicker, ColorStrip,
    ComboBox, Configure, Corners, DragValue, Modal, Panel, ProgressBar, Rect, RgbaF32, Shadow,
    Sizing, Slider, Spinner, SrgbaU8, Stroke, Switch, Text, ToggleTheme,
};

use crate::fixtures::{DARK_BG, SRGB_ROUND_TRIP, assert_px};

use crate::golden_name::GoldenName;
use crate::goldens::assert_matches_golden;

use crate::harness::{FIXTURE_PALETTE, Harness};

#[test]
fn button_hello_matches_golden() {
    let mut h = Harness::new();
    let img = h
        .size(UVec2::new(256, 96))
        .frame(|ui| {
            Button::new()
                .auto_id()
                .label("hello")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui);
        })
        .image;
    assert_matches_golden(GoldenName::ButtonHello, &img);
}

/// Rounded-rect SDF AA: solid fill, border, corner radius, in a darker scene.
#[test]
fn frame_filled_with_border_matches_golden() {
    let mut h = Harness::new();
    let img = h
        .size(UVec2::new(220, 140))
        .frame(|ui| {
            Panel::vstack()
                .auto_id()
                .padding(20.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Block::new()
                        .id_salt("card")
                        .size((Sizing::FILL, Sizing::FILL))
                        .background(Background {
                            fill: RgbaF32::srgb(0.20, 0.30, 0.55).into(),
                            border: Stroke::new(RgbaF32::srgb(0.65, 0.80, 1.00), 2.0),
                            corners: Corners::all(16.0),
                            shadow: Shadow::NONE,
                        })
                        .show(ui);
                });
        })
        .image;
    assert_matches_golden(GoldenName::FrameFilledWithBorder, &img);
}

/// A border paints inside its rect. On a pixel-aligned rect SDF coverage is
/// exactly 0 or 1, so the middle row reads: clear, border for its width, fill.
#[test]
fn a_border_paints_inside_the_rect() {
    let mut h = Harness::new();
    let img = h
        .size(UVec2::new(20, 20))
        .clear(RgbaF32::BLACK)
        .frame(|ui| {
            Panel::zstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    ui.add_shape(
                        Shape::rect(Rect::new(4.0, 4.0, 12.0, 12.0))
                            .fill(RgbaF32::srgb(1.0, 0.0, 0.0))
                            .border(Stroke::new(RgbaF32::WHITE, 2.0)),
                    );
                });
        })
        .image;
    let clear = Rgba([0, 0, 0, 255]);
    let border = Rgba([255, 255, 255, 255]);
    let fill = Rgba([255, 0, 0, 255]);
    let expected: Vec<_> = (0..20)
        .map(|x| match x {
            4..=5 | 14..=15 => border,
            6..=13 => fill,
            _ => clear,
        })
        .collect();
    let row: Vec<_> = (0..20).map(|x| *img.get_pixel(x, 10)).collect();
    assert_eq!(row, expected);
}

/// Rounded-clip stencil path: pink canvas, a rounded panel (distinct corner
/// radii, 1px black stroke), then a black child whose square corners the
/// stencil must trim. Distinct radii catch corner-mixing bugs.
#[test]
fn surface_rounded_clips_full_fill_child() {
    let mut h = Harness::new();
    let pink = RgbaF32::srgb(1.0, 0.42, 0.72);
    let black = RgbaF32::srgb(0.0, 0.0, 0.0);
    let img = h
        .size(UVec2::new(220, 220))
        .frame(|ui| {
            Panel::vstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .padding(20.0)
                .background(Background {
                    fill: pink.into(),
                    ..Default::default()
                })
                .show(ui, |ui| {
                    Panel::zstack()
                        .id_salt("rounded")
                        .size((Sizing::FILL, Sizing::FILL))
                        .background(Background {
                            fill: RgbaF32::TRANSPARENT.into(),
                            border: Stroke::new(RgbaF32::from_srgba(SrgbaU8::rgb(0, 255, 0)), 5.0),
                            corners: Corners::new(4.0, 12.0, 20.0, 28.0),
                            shadow: Shadow::NONE,
                        })
                        .clip_rounded()
                        .show(ui, |ui| {
                            Block::new()
                                .id_salt("inner")
                                .size((Sizing::FILL, Sizing::FILL))
                                .background(Background {
                                    fill: black.into(),
                                    ..Default::default()
                                })
                                .show(ui);
                        });
                });
        })
        .image;
    assert_matches_golden(GoldenName::SurfaceRoundedClipsFullFillChild, &img);
}

/// Regression: a rounded clip extending off-screen on every side. The mask
/// SDF must use the panel's true rect, not the viewport-clamped scissor.
///
/// Panel `(-6, -6) .. (194, 144)` over `120x90`, radius 24: only the TL arc
/// (centre `(18, 18)`) is visible, with a `DARK_BG` cutout there. The other
/// corners must be solid black; a clamped mask notches them.
#[test]
fn rounded_clip_partially_offscreen_does_not_bleed_corners() {
    let mut h = Harness::new();
    let img = h
        .size(UVec2::new(120, 90))
        .frame(|ui| {
            Panel::canvas()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Panel::zstack()
                        .id_salt("rounded")
                        .position(Vec2::new(-6.0, -6.0))
                        .size((Sizing::fixed(200.0), Sizing::fixed(150.0)))
                        .background(Background {
                            fill: RgbaF32::TRANSPARENT.into(),
                            border: Stroke::new(RgbaF32::from_srgba(SrgbaU8::rgb(0, 255, 0)), 4.0),
                            corners: Corners::all(24.0),
                            shadow: Shadow::NONE,
                        })
                        .clip_rounded()
                        .show(ui, |ui| {
                            Block::new()
                                .id_salt("inner")
                                .size((Sizing::FILL, Sizing::FILL))
                                .background(Background {
                                    fill: RgbaF32::srgb(0.0, 0.0, 0.0).into(),
                                    ..Default::default()
                                })
                                .show(ui);
                        });
                });
        })
        .image;

    // sRGB(0.08, 0.08, 0.10) ~ (20, 20, 25); near-black means all channels
    // well under that.
    let is_near_black = |p: Rgba<u8>| p.0[0] < 8 && p.0[1] < 8 && p.0[2] < 8;
    let is_dark_bg = |p: Rgba<u8>| p.0[0] > 12 && p.0[0] < 32 && p.0[2] > 12 && p.0[2] < 40;

    // The TL corner is the genuine cutout.
    let tl = *img.get_pixel(0, 0);
    assert!(
        is_dark_bg(tl),
        "TL viewport corner should be DARK_BG (genuine offscreen-arc cutout), got rgba={:?}",
        tl.0,
    );

    // The other three corners must be solid black.
    for (x, y, label) in [(119, 0, "TR"), (0, 89, "BL"), (119, 89, "BR")] {
        let px = *img.get_pixel(x, y);
        assert!(
            is_near_black(px),
            "{label} viewport corner ({x},{y}) should be black (panel arc is offscreen), \
             got rgba={:?} — the mask SDF is using the viewport-clamped scissor \
             instead of the panel's true rect.",
            px.0,
        );
    }

    let centre = *img.get_pixel(60, 45);
    assert!(
        is_near_black(centre),
        "viewport centre should be solid black, got rgba={:?}",
        centre.0,
    );

    assert_matches_golden(GoldenName::RoundedClipPartiallyOffscreen, &img);
}

/// The backbuffer rebuild on a size change must reset the stencil attachment,
/// else wgpu validation panics on the size mismatch. Two rounded-clip renders
/// at different sizes, probed where the clip decides the pixel.
#[test]
fn rounded_clip_survives_surface_resize() {
    let mut h = Harness::new();
    let scene = |ui: &mut palantir::Ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .padding(10.0)
            .show(ui, |ui| {
                Panel::zstack()
                    .id_salt("rounded")
                    .size((Sizing::FILL, Sizing::FILL))
                    .background(Background {
                        fill: RgbaF32::srgb(0.2, 0.2, 0.3).into(),
                        corners: Corners::all(8.0),
                        ..Default::default()
                    })
                    .clip_rounded()
                    .show(ui, |_| {});
            });
    };
    // A stale stencil would mismatch the pass and panic. Each render must still
    // draw: corner (10, 10) is 7.5*sqrt(2) ~ 10.6 px from the arc centre (18, 18),
    // outside the 8 px arc, so it reads clear; the centre reads the fill.
    let clear = DARK_BG.to_srgba_u8();
    let fill = RgbaF32::srgb(0.2, 0.2, 0.3).to_srgba_u8();
    for size in [UVec2::new(120, 120), UVec2::new(240, 200)] {
        let img = h.size(size).frame(scene).image;
        assert_px(
            img.get_pixel(10, 10).0,
            [clear.r, clear.g, clear.b, 255],
            SRGB_ROUND_TRIP,
            format_args!("{size}: the corner outside the arc"),
        );
        assert_px(
            img.get_pixel(size.x / 2, size.y / 2).0,
            [fill.r, fill.g, fill.b, 255],
            SRGB_ROUND_TRIP,
            format_args!("{size}: the panel's centre"),
        );
    }
}

/// ProgressBar at 50%: half-width accent fill over the pill track.
#[test]
fn progress_bar_half_matches_golden() {
    let mut h = Harness::new();
    let img = h
        .size(UVec2::new(220, 60))
        .frame(|ui| {
            Panel::vstack()
                .auto_id()
                .padding(20.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    ProgressBar::new(0.5).id_salt("pb").show(ui);
                });
        })
        .image;
    assert_matches_golden(GoldenName::ProgressBarHalf, &img);
}

/// Switch on and off, animation disabled: knob rest positions and the
/// `Canvas` track with `.position`ed knob.
#[test]
fn toggle_switch_states_matches_golden() {
    let mut h = Harness::new();
    let mut style = ToggleTheme::switch(&FIXTURE_PALETTE);
    style.defaults.animation = None; // sit at the rest position, no first-frame transient
    let img = h
        .size(UVec2::new(220, 110))
        .frame(|ui| {
            let mut on = true;
            let mut off = false;
            Panel::vstack()
                .auto_id()
                .padding(20.0)
                .gap(16.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Switch::new(&mut on)
                        .id_salt("on")
                        .label("on")
                        .style(&style)
                        .show(ui);
                    Switch::new(&mut off)
                        .id_salt("off")
                        .label("off")
                        .style(&style)
                        .show(ui);
                });
        })
        .image;
    assert_matches_golden(GoldenName::ToggleSwitchStates, &img);
}

/// Spinner at phase 0: a round-capped arc fading from transparent tail to head.
#[test]
fn spinner_matches_golden() {
    let mut h = Harness::new();
    let img = h
        .size(UVec2::new(80, 80))
        .frame(|ui| {
            Panel::vstack()
                .auto_id()
                .padding(16.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Spinner::new().diameter(48.0).id_salt("sp").show(ui);
                });
        })
        .image;
    assert_matches_golden(GoldenName::Spinner, &img);
}

/// Slider at 30%: two-tone track split at the knob via `Fill` weights.
#[test]
fn slider_thirty_percent_matches_golden() {
    let mut h = Harness::new();
    let img = h
        .size(UVec2::new(240, 60))
        .frame(|ui| {
            let mut v = 0.3_f64;
            Panel::vstack()
                .auto_id()
                .padding(20.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Slider::new(&mut v, 0.0..=1.0).id_salt("sl").show(ui);
                });
        })
        .image;
    assert_matches_golden(GoldenName::SliderThirtyPercent, &img);
}

#[test]
fn drag_value_matches_golden() {
    let mut h = Harness::new();
    let img = h
        .size(UVec2::new(140, 64))
        .frame(|ui| {
            let mut v = 42.5_f64;
            Panel::vstack()
                .auto_id()
                .padding(16.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    DragValue::new(&mut v)
                        .decimals(1)
                        .suffix(" px")
                        .size((Sizing::fixed(100.0), Sizing::HUG))
                        .id_salt("dv")
                        .show(ui);
                });
        })
        .image;
    assert_matches_golden(GoldenName::DragValue, &img);
}

/// ComboBox (closed): trigger with the choice and a polyline chevron
/// (font-independent), right via `SpaceBetween`.
#[test]
fn combo_box_closed_matches_golden() {
    let mut h = Harness::new();
    let opts = ["Apple", "Banana", "Cherry"];
    let img = h
        .size(UVec2::new(220, 70))
        .frame(|ui| {
            let mut sel = 1usize;
            Panel::vstack()
                .auto_id()
                .padding(16.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    ComboBox::new(&mut sel, &opts)
                        .size((Sizing::fixed(160.0), Sizing::HUG))
                        .id_salt("cb")
                        .show(ui);
                });
        })
        .image;
    assert_matches_golden(GoldenName::ComboBoxClosed, &img);
}

/// Modal: a centered card over the dim backdrop in `Layer::Modal`.
#[test]
fn modal_dialog_matches_golden() {
    let mut h = Harness::new();
    let img = h
        .size(UVec2::new(300, 200))
        .frame(|ui| {
            Panel::vstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .background(Background {
                    fill: RgbaF32::srgb(0.35, 0.45, 0.65).into(),
                    border: Stroke::NONE,
                    corners: Corners::ZERO,
                    shadow: Shadow::NONE,
                })
                .show(ui, |_| {});
            Modal::new().id_salt("m").show(ui, |ui, _| {
                Text::new("Confirm?").id_salt("mt").show(ui);
            });
        })
        .image;
    assert_matches_golden(GoldenName::ModalDialog, &img);
}

/// The colour field and both bars at one hue, in each model. The only
/// end-to-end check that the CPU texture reaches the screen.
#[test]
fn color_field_and_bars_match_golden() {
    let mut h = Harness::new();
    let img = h
        .size(UVec2::new(480, 220))
        .frame(|ui| {
            Panel::hstack()
                .auto_id()
                .padding(12.0)
                .gap(16.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    for model in ColorModel::ALL {
                        let mut coords = ColorCoords::new(model, RgbaF32::hex(0x2b7fd4), 0.0);
                        Panel::vstack()
                            .id_salt(model.label())
                            .gap(8.0)
                            .size((Sizing::HUG, Sizing::HUG))
                            .show(ui, |ui| {
                                ColorField::new(&mut coords).id_salt("field").show(ui);
                                ColorStrip::for_hue(&mut coords).id_salt("hue").show(ui);
                                let mut translucent = coords.to_color().with_alpha(0.6);
                                ColorStrip::for_alpha(&mut translucent)
                                    .id_salt("alpha")
                                    .show(ui);
                            });
                    }
                });
        })
        .image;
    assert_matches_golden(GoldenName::ColorFieldAndBars, &img);
}

/// The whole panel: field, bars, preview chip, channel values, model switch,
/// preset row.
#[test]
fn color_picker_panel_matches_golden() {
    let mut h = Harness::new();
    let img = h
        .size(UVec2::new(280, 500))
        .frame(|ui| {
            let mut color = RgbaF32::hex(0x4cd3ff).with_alpha(0.75);
            Panel::vstack()
                .auto_id()
                .padding(12.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    ColorPicker::new(&mut color)
                        .alpha(true)
                        .history(true)
                        .id_salt("picker")
                        .show(ui);
                });
        })
        .image;
    assert_matches_golden(GoldenName::ColorPickerPanel, &img);
}

/// The focus ring after a Tab press: on the first card's rounded edge and
/// border, on no other. The second card has no chrome, so its edge is where the
/// ring would paint.
#[test]
fn focus_ring_matches_golden() {
    use palantir::{InputEvent, Key, KeyText, Ui};

    let mut h = Harness::new();
    let scene = |ui: &mut Ui| {
        Panel::hstack()
            .auto_id()
            .padding(16.0)
            .gap(16.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Block::new()
                    .id_salt("ringed")
                    .size((Sizing::FILL, Sizing::FILL))
                    .focusable(true)
                    .background(Background {
                        fill: RgbaF32::srgb(0.20, 0.30, 0.55).into(),
                        border: Stroke::new(RgbaF32::srgb(0.65, 0.80, 1.00), 1.0),
                        corners: Corners::all(10.0),
                        shadow: Shadow::NONE,
                    })
                    .show(ui);
                Block::new()
                    .id_salt("plain")
                    .size((Sizing::FILL, Sizing::FILL))
                    .focusable(true)
                    .show(ui);
            });
    };
    h.size(UVec2::new(220, 100)).frame(scene);
    h.host.on_input(InputEvent::KeyDown {
        key: Key::Tab,
        repeat: false,
        physical: Key::Tab,
        text: KeyText::EMPTY,
    });
    let img = h.frame(scene).image;
    assert_matches_golden(GoldenName::FocusRing, &img);
}
