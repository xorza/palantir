//! Per-widget fixtures: the smallest scene exercising one widget's render path.

use glam::{UVec2, Vec2};
use palantir::golden::image::Rgba;
use palantir::widget::Shape;
use palantir::{
    Background, Block, Button, ColorCoords, ColorField, ColorModel, ColorPicker, ColorStrip,
    ComboBox, Configure, Corners, DragValue, InputEvent, Key, KeyText, Modal, Panel, ProgressBar,
    Rect, RgbaF32, Sizing, Slider, Spinner, SrgbaU8, Stroke, Switch, Text, ToggleTheme, Ui,
};

use crate::fixtures::{DARK_BG, SRGB_ROUND_TRIP, assert_px, canvas};
use crate::golden_name::GoldenName;
use crate::goldens::{assert_matches_golden, assert_scene_matches_golden};
use crate::harness::{FIXTURE_PALETTE, Harness};

/// A well padded by `p` and filling the surface around `body`.
fn padded(ui: &mut Ui, p: f32, body: impl FnOnce(&mut Ui)) {
    Panel::vstack()
        .id_salt("well")
        .padding(p)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, body);
}

/// `panel` with a transparent fill and a green border, clipping its black child to `corners`.
fn rounded_clip(ui: &mut Ui, panel: Panel, border: f32, corners: Corners) {
    panel
        .background(
            Background::rounded(RgbaF32::TRANSPARENT, corners).with_border(Stroke::new(
                RgbaF32::from_srgba(SrgbaU8::rgb(0, 255, 0)),
                border,
            )),
        )
        .clip_rounded()
        .show(ui, |ui| {
            Block::new()
                .id_salt("inner")
                .size((Sizing::FILL, Sizing::FILL))
                .background(Background::fill(RgbaF32::BLACK))
                .show(ui);
        });
}

#[test]
fn button_hello_matches_golden() {
    assert_scene_matches_golden(GoldenName::ButtonHello, UVec2::new(256, 96), |ui| {
        Button::new()
            .auto_id()
            .label("hello")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui);
    });
}

/// Rounded-rect SDF AA: solid fill, border, corner radius, in a darker scene.
#[test]
fn frame_filled_with_border_matches_golden() {
    assert_scene_matches_golden(
        GoldenName::FrameFilledWithBorder,
        UVec2::new(220, 140),
        |ui| {
            padded(ui, 20.0, |ui| {
                Block::new()
                    .id_salt("card")
                    .size((Sizing::FILL, Sizing::FILL))
                    .background(
                        Background::rounded(RgbaF32::srgb(0.20, 0.30, 0.55), Corners::all(16.0))
                            .with_border(Stroke::new(RgbaF32::srgb(0.65, 0.80, 1.00), 2.0)),
                    )
                    .show(ui);
            });
        },
    );
}

/// A border paints inside its rect. On a pixel-aligned rect SDF coverage is
/// exactly 0 or 1, so the middle row reads: clear, border for its width, fill.
#[test]
fn a_border_paints_inside_the_rect() {
    let img = Harness::new()
        .size(UVec2::new(20, 20))
        .clear(RgbaF32::BLACK)
        .frame(|ui| {
            canvas(ui, |ui| {
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
/// radii, 5 px green border), then a black child whose square corners the
/// stencil must trim. Distinct radii catch corner-mixing bugs.
#[test]
fn surface_rounded_clips_full_fill_child() {
    assert_scene_matches_golden(
        GoldenName::SurfaceRoundedClipsFullFillChild,
        UVec2::new(220, 220),
        |ui| {
            Panel::vstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .padding(20.0)
                .background(Background::fill(RgbaF32::srgb(1.0, 0.42, 0.72)))
                .show(ui, |ui| {
                    let panel = Panel::zstack()
                        .id_salt("rounded")
                        .size((Sizing::FILL, Sizing::FILL));
                    rounded_clip(ui, panel, 5.0, Corners::new(4.0, 12.0, 20.0, 28.0));
                });
        },
    );
}

/// Regression: a rounded clip extending off-screen on every side. The mask
/// SDF must use the panel's true rect, not the viewport-clamped scissor.
///
/// Panel `(-6, -6) .. (194, 144)` over `120x90`, radius 24: only the TL arc
/// (centre `(18, 18)`) is visible, with a `DARK_BG` cutout there. The other
/// corners must be solid black; a clamped mask notches them.
#[test]
fn rounded_clip_partially_offscreen_does_not_bleed_corners() {
    let img = Harness::new()
        .size(UVec2::new(120, 90))
        .frame(|ui| {
            canvas(ui, |ui| {
                let panel = Panel::zstack()
                    .id_salt("rounded")
                    .position(Vec2::new(-6.0, -6.0))
                    .size((Sizing::fixed(200.0), Sizing::fixed(150.0)));
                rounded_clip(ui, panel, 4.0, Corners::all(24.0));
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
    let scene = |ui: &mut Ui| {
        padded(ui, 10.0, |ui| {
            Panel::zstack()
                .id_salt("rounded")
                .size((Sizing::FILL, Sizing::FILL))
                .background(Background::rounded(
                    RgbaF32::srgb(0.2, 0.2, 0.3),
                    Corners::all(8.0),
                ))
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
    assert_scene_matches_golden(GoldenName::ProgressBarHalf, UVec2::new(220, 60), |ui| {
        padded(ui, 20.0, |ui| {
            ProgressBar::new(0.5).id_salt("pb").show(ui);
        });
    });
}

/// Switch on and off, animation disabled: knob rest positions and the
/// `Canvas` track with `.position`ed knob.
#[test]
fn toggle_switch_states_matches_golden() {
    let mut style = ToggleTheme::switch(&FIXTURE_PALETTE);
    // At the rest position, with no first-frame transient.
    style.defaults.animation = None;
    assert_scene_matches_golden(GoldenName::ToggleSwitchStates, UVec2::new(220, 110), |ui| {
        Panel::vstack()
            .auto_id()
            .padding(20.0)
            .gap(16.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                for (salt, mut on) in [("on", true), ("off", false)] {
                    Switch::new(&mut on)
                        .id_salt(salt)
                        .label(salt)
                        .style(&style)
                        .show(ui);
                }
            });
    });
}

/// Spinner at phase 0: a round-capped arc fading from transparent tail to head.
#[test]
fn spinner_matches_golden() {
    assert_scene_matches_golden(GoldenName::Spinner, UVec2::new(80, 80), |ui| {
        padded(ui, 16.0, |ui| {
            Spinner::new().diameter(48.0).id_salt("sp").show(ui);
        });
    });
}

/// Slider at 30%: two-tone track split at the knob via `Fill` weights.
#[test]
fn slider_thirty_percent_matches_golden() {
    assert_scene_matches_golden(GoldenName::SliderThirtyPercent, UVec2::new(240, 60), |ui| {
        padded(ui, 20.0, |ui| {
            Slider::new(&mut 0.3, 0.0..=1.0).id_salt("sl").show(ui);
        });
    });
}

#[test]
fn drag_value_matches_golden() {
    assert_scene_matches_golden(GoldenName::DragValue, UVec2::new(140, 64), |ui| {
        padded(ui, 16.0, |ui| {
            DragValue::new(&mut 42.5)
                .decimals(1)
                .suffix(" px")
                .size((Sizing::fixed(100.0), Sizing::HUG))
                .id_salt("dv")
                .show(ui);
        });
    });
}

/// ComboBox (closed): trigger with the choice and a polyline chevron
/// (font-independent), right via `SpaceBetween`.
#[test]
fn combo_box_closed_matches_golden() {
    let opts = ["Apple", "Banana", "Cherry"];
    assert_scene_matches_golden(GoldenName::ComboBoxClosed, UVec2::new(220, 70), |ui| {
        padded(ui, 16.0, |ui| {
            ComboBox::new(&mut 1, &opts)
                .size((Sizing::fixed(160.0), Sizing::HUG))
                .id_salt("cb")
                .show(ui);
        });
    });
}

/// Modal: a centered card over the dim backdrop in `Layer::Modal`.
#[test]
fn modal_dialog_matches_golden() {
    assert_scene_matches_golden(GoldenName::ModalDialog, UVec2::new(300, 200), |ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .background(Background::fill(RgbaF32::srgb(0.35, 0.45, 0.65)))
            .show(ui, |_| {});
        Modal::new().id_salt("m").show(ui, |ui, _| {
            Text::new("Confirm?").id_salt("mt").show(ui);
        });
    });
}

/// The colour field and both bars at one hue, in each model. The only
/// end-to-end check that the CPU texture reaches the screen.
#[test]
fn color_field_and_bars_match_golden() {
    assert_scene_matches_golden(GoldenName::ColorFieldAndBars, UVec2::new(480, 220), |ui| {
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
    });
}

/// The whole panel: field, bars, preview chip, channel values, model switch,
/// preset row.
#[test]
fn color_picker_panel_matches_golden() {
    assert_scene_matches_golden(GoldenName::ColorPickerPanel, UVec2::new(280, 500), |ui| {
        let mut color = RgbaF32::hex(0x4cd3ff).with_alpha(0.75);
        padded(ui, 12.0, |ui| {
            ColorPicker::new(&mut color)
                .alpha(true)
                .history(true)
                .id_salt("picker")
                .show(ui);
        });
    });
}

/// The focus ring after a Tab press: on the first card's rounded edge and
/// border, on no other. The second card has no chrome, so its edge is where the
/// ring would paint.
#[test]
fn focus_ring_matches_golden() {
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
                    .background(
                        Background::rounded(RgbaF32::srgb(0.20, 0.30, 0.55), Corners::all(10.0))
                            .with_border(Stroke::new(RgbaF32::srgb(0.65, 0.80, 1.00), 1.0)),
                    )
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
