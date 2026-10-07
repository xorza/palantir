//! Gradients end to end: an overflowing LUT atlas, the linear, radial and
//! conic shader paths, and the showcase gradients page.

use glam::UVec2;
use palantir::golden::image::RgbaImage;
use palantir::widget::Shape;
use palantir::{
    Background, Block, Brush, Configure, ConicGradient, Corners, LinearGradient, Panel,
    RadialGradient, Rect, RgbaF32, Sizing,
};
use std::f32::consts::FRAC_PI_2;

use crate::fixtures::SRGB_ROUND_TRIP;
use crate::golden_name::GoldenName;
use crate::goldens::assert_matches_golden;
use crate::harness::Harness;
use crate::support;

/// More distinct gradients than the atlas's 256 initial rows, forcing growth.
const COLS: u32 = 20;

const ROWS: u32 = 16;

const SWATCHES: u32 = COLS * ROWS;

const SWATCH: u32 = 8;

const VIEWPORT: UVec2 = UVec2::new(COLS * SWATCH, ROWS * SWATCH);

const CLEAR: RgbaF32 = RgbaF32::BLACK;

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

/// Each swatch's two stops share one colour, so its LUT row is flat and a
/// wrong row shows as a neighbouring swatch's colour.
fn render_swatches() -> RgbaImage {
    let mut harness = Harness::new();
    harness
        .size(VIEWPORT)
        .clear(CLEAR)
        .frame(|ui| {
            Panel::canvas()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
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
        .image
}

/// 320 gradients in one frame force the atlas to grow, the backend to resize
/// its LUT, and the shaders to read the new height via `textureDimensions`.
///
/// Asserted per swatch, since a permuted row assignment would still satisfy
/// mutual distinctness.
#[test]
fn overflowing_gradient_atlas_paints_every_swatch() {
    let img = render_swatches();
    for i in 0..SWATCHES {
        let want = swatch_color(i).to_srgba_u8();
        // Clear of the edge AA on the quad boundary.
        let x = (i % COLS) * SWATCH + SWATCH / 2;
        let y = (i / COLS) * SWATCH + SWATCH / 2;
        let got = img.get_pixel(x, y).0;
        let delta = [
            got[0].abs_diff(want.r),
            got[1].abs_diff(want.g),
            got[2].abs_diff(want.b),
        ];
        // The LUT is `f16`, finer than an sRGB step, so only encode rounding
        // remains; swatches are at least 5 sRGB units apart.
        assert!(
            delta.iter().all(|&d| d <= SRGB_ROUND_TRIP),
            "swatch {i} at ({x}, {y}): got {got:?}, want [{}, {}, {}, 255] (delta {delta:?})",
            want.r,
            want.g,
            want.b,
        );
        assert_eq!(got[3], 255, "swatch {i} must be opaque");
    }
}

/// Golden of the same scene.
#[test]
fn overflowing_gradient_atlas_matches_golden() {
    assert_matches_golden(GoldenName::OverflowingGradientAtlas, &render_swatches());
}

/// The LUT is sampled at texel centres: parameter `t` reads the point
/// `t * 255` between them.
///
/// A hard stop at 0.25 is stored at 64/255. Pixel 258 of 1024 sits at
/// `t = 258.5 / 1024`, `255 * t - 64 = 0.373` of the way to blue: red 0.627
/// and blue 0.373 linear, sRGB 207 and 163.
#[test]
fn a_gradient_samples_its_lut_at_texel_centres() {
    use palantir::Stop;

    let red = RgbaF32::new(1.0, 0.0, 0.0, 1.0);
    let blue = RgbaF32::new(0.0, 0.0, 1.0, 1.0);
    let mut h = Harness::new();
    let img = h
        .size(UVec2::new(1024, 16))
        .clear(RgbaF32::BLACK)
        .frame(|ui| {
            Panel::canvas()
                .id_salt("hard-stop")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
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
    let img = Harness::new()
        .size(UVec2::new(560, 1180))
        .frame(|ui| {
            Panel::vstack()
                .auto_id()
                .gap(support::PAGE_GAP)
                .show(ui, showcase_page::build);
        })
        .image;
    assert_matches_golden(GoldenName::ShowcaseGradientsPage, &img);
}

/// Pins the linear-gradient paint path end to end with a vertical two-stop
/// gradient.
#[test]
fn frame_linear_gradient_matches_golden() {
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
                            fill: Brush::Linear(LinearGradient::two_stop(
                                FRAC_PI_2,
                                RgbaF32::hex(0x1a1a2e),
                                RgbaF32::hex(0x4c5cdb),
                            )),
                            corners: Corners::all(16.0),
                            ..Default::default()
                        })
                        .show(ui);
                });
        })
        .image;
    assert_matches_golden(GoldenName::FrameLinearGradient, &img);
}

/// A rounded rect with a linear-gradient fill paints correctly.
#[test]
fn add_shape_rounded_rect_linear_gradient_matches_golden() {
    let mut h = Harness::new();
    let img = h
        .size(UVec2::new(220, 140))
        .frame(|ui| {
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
        })
        .image;
    assert_matches_golden(GoldenName::AddShapeRoundedRectLinearGradient, &img);
}

/// Pins the radial and conic shader paths with two side-by-side frames.
#[test]
fn radial_and_conic_gradient_matches_golden() {
    let mut h = Harness::new();
    let img = h
        .size(UVec2::new(320, 160))
        .frame(|ui| {
            Panel::hstack()
                .auto_id()
                .gap(16.0)
                .padding(16.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    let r =
                        RadialGradient::two_stop(RgbaF32::hex(0xfacc15), RgbaF32::hex(0x1a1a2e));
                    Block::new()
                        .id_salt("radial")
                        .size((Sizing::FILL, Sizing::FILL))
                        .background(Background {
                            fill: Brush::Radial(r),
                            corners: Corners::all(8.0),
                            ..Default::default()
                        })
                        .show(ui);
                    let c = ConicGradient::new(
                        glam::Vec2::splat(0.5),
                        0.0,
                        [
                            palantir::Stop::new(0.0, RgbaF32::hex(0xff5e44)),
                            palantir::Stop::new(0.25, RgbaF32::hex(0xfacc15)),
                            palantir::Stop::new(0.5, RgbaF32::hex(0x46c46c)),
                            palantir::Stop::new(0.75, RgbaF32::hex(0x4c5cdb)),
                            palantir::Stop::new(1.0, RgbaF32::hex(0xff5e44)),
                        ],
                    );
                    Block::new()
                        .id_salt("conic")
                        .size((Sizing::FILL, Sizing::FILL))
                        .background(Background {
                            fill: Brush::Conic(c),
                            corners: Corners::all(8.0),
                            ..Default::default()
                        })
                        .show(ui);
                });
        })
        .image;
    assert_matches_golden(GoldenName::RadialAndConicGradient, &img);
}
