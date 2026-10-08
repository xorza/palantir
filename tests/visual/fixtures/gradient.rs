//! Gradients end to end: an overflowing LUT atlas, LUT sampling, a shape's
//! gradient fill, and the showcase gradients page, whose tiles pin the linear,
//! radial and conic shader paths on chrome.

use glam::UVec2;
use palantir::widget::Shape;
use palantir::{Configure, LinearGradient, Panel, Rect, RgbaF32, Sizing, Stop};

use crate::fixtures::{SRGB_ROUND_TRIP, assert_px, canvas};
use crate::golden_name::GoldenName;
use crate::goldens::assert_scene_matches_golden;
use crate::harness::Harness;
use crate::support;

/// More distinct gradients than the atlas's 256 initial rows, forcing growth.
const COLS: u32 = 20;

const ROWS: u32 = 16;

const SWATCHES: u32 = COLS * ROWS;

const SWATCH: u32 = 8;

const VIEWPORT: UVec2 = UVec2::new(COLS * SWATCH, ROWS * SWATCH);

/// Stop colour for swatch `i`, in linear light as `byte / 255`, spread wide
/// enough that a wrong LUT row can't pass as rounding.
fn swatch_color(i: u32) -> RgbaF32 {
    let lin = |byte: u32| byte as f32 / 255.0;
    RgbaF32::new(
        lin(40 + (i % COLS) * 10),
        lin(40 + (i / COLS) * 12),
        lin(200),
        1.0,
    )
}

/// 320 gradients in one frame force the atlas to grow, the backend to resize
/// its LUT, and the shaders to read the new height via `textureDimensions`.
///
/// Each swatch's two stops share one colour, so its LUT row is flat and a
/// wrong row shows as a neighbouring swatch's colour. Every pixel of every
/// swatch is asserted: a permuted row assignment would still satisfy mutual
/// distinctness, and the swatches sit on whole pixels, so none is partly
/// covered.
#[test]
fn overflowing_gradient_atlas_paints_every_swatch() {
    let img = Harness::new()
        .size(VIEWPORT)
        .clear(RgbaF32::BLACK)
        .frame(|ui| {
            canvas(ui, |ui| {
                for i in 0..SWATCHES {
                    let color = swatch_color(i);
                    let rect = Rect::new(
                        ((i % COLS) * SWATCH) as f32,
                        ((i / COLS) * SWATCH) as f32,
                        SWATCH as f32,
                        SWATCH as f32,
                    );
                    ui.add_shape(
                        Shape::rect(rect).fill(LinearGradient::two_stop(0.0, color, color)),
                    );
                }
            });
        })
        .image;
    for (x, y, pixel) in img.enumerate_pixels() {
        let i = y / SWATCH * COLS + x / SWATCH;
        let want = swatch_color(i).to_srgba_u8();
        // The LUT is `f16`, finer than an sRGB step, so only encode rounding
        // remains; swatches are at least 5 sRGB units apart.
        assert_px(
            pixel.0,
            [want.r, want.g, want.b, 255],
            SRGB_ROUND_TRIP,
            format_args!("swatch {i} at ({x}, {y})"),
        );
    }
}

/// The LUT is sampled at texel centres: parameter `t` reads the point
/// `t * 255` between them.
///
/// A hard stop at 0.25 is stored at 64/255. Pixel 258 of 1024 sits at
/// `t = 258.5 / 1024`, `255 * t - 64 = 0.373` of the way to blue: red 0.627
/// and blue 0.373 linear, sRGB 207 and 163.
#[test]
fn a_gradient_samples_its_lut_at_texel_centres() {
    let red = RgbaF32::new(1.0, 0.0, 0.0, 1.0);
    let blue = RgbaF32::new(0.0, 0.0, 1.0, 1.0);
    let mut h = Harness::new();
    let img = h
        .size(UVec2::new(1024, 16))
        .clear(RgbaF32::BLACK)
        .frame(|ui| {
            canvas(ui, |ui| {
                ui.add_shape(Shape::rect(Rect::new(0.0, 0.0, 1024.0, 16.0)).fill(
                    LinearGradient::new(0.0, [Stop::new(0.25, red), Stop::new(0.25, blue)]),
                ));
            });
        })
        .image;
    let [r, g, b, _] = img.get_pixel(258, 8).0;
    assert!(
        r.abs_diff(207) <= SRGB_ROUND_TRIP && g == 0 && b.abs_diff(163) <= SRGB_ROUND_TRIP,
        "pixel 258 is {r}, {g}, {b}; texel centres put it at 207, 0, 163",
    );
}

/// The showcase's gradients page, compiled from the example.
#[path = "../../../examples/showcase/pages/gradients.rs"]
mod showcase_page;

/// The showcase's gradients page as one golden.
#[test]
fn showcase_gradients_page_matches_golden() {
    assert_scene_matches_golden(
        GoldenName::ShowcaseGradientsPage,
        UVec2::new(560, 1180),
        |ui| {
            Panel::vstack()
                .auto_id()
                .gap(support::PAGE_GAP)
                .show(ui, showcase_page::build);
        },
    );
}

/// A rounded rect with a linear-gradient fill paints correctly.
#[test]
fn add_shape_rounded_rect_linear_gradient_matches_golden() {
    assert_scene_matches_golden(
        GoldenName::AddShapeRoundedRectLinearGradient,
        UVec2::new(220, 140),
        |ui| {
            Panel::vstack()
                .auto_id()
                .padding(20.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    ui.add_shape(
                        Shape::rect(Rect::new(0.0, 0.0, 180.0, 100.0))
                            .corners(12.0)
                            .fill(LinearGradient::two_stop(
                                0.0,
                                RgbaF32::hex(0xff5e44),
                                RgbaF32::hex(0xfacc15),
                            )),
                    );
                });
        },
    );
}
