//! Gradients end to end: a frame authoring more distinct gradients than
//! the LUT atlas holds still paints every one correctly, the linear,
//! radial and conic shader paths match their goldens, and so does the
//! showcase's gradients page.

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

/// More distinct gradients than the atlas's 256 initial rows (255
/// usable), so one frame forces it to grow. 20 × 16 swatches.
const COLS: u32 = 20;

const ROWS: u32 = 16;

const SWATCHES: u32 = COLS * ROWS;

const SWATCH: u32 = 8;

const VIEWPORT: UVec2 = UVec2::new(COLS * SWATCH, ROWS * SWATCH);

const CLEAR: RgbaF32 = RgbaF32::BLACK;

/// Stop colour for swatch `i`, written in linear light as `byte / 255`.
/// Channels are spread far enough apart that neighbouring swatches stay
/// distinguishable after the sRGB framebuffer encode, so sampling the
/// wrong LUT row can't pass as rounding.
fn swatch_color(i: u32) -> RgbaF32 {
    let lin = |byte: u32| byte as f32 / 255.0;
    RgbaF32::new(
        lin(40 + (i % COLS) * 10),
        lin(40 + (i / COLS) * 12),
        lin(200),
        1.0,
    )
}

/// Each swatch is a two-stop gradient whose stops share one colour, so
/// its whole LUT row bakes to that flat colour and the swatch paints
/// it uniformly. Stops are the atlas key, so all `SWATCHES` gradients
/// are distinct rows — and because a row is flat, reading the *wrong*
/// row shows up as a neighbouring swatch's colour rather than a subtle
/// interpolation shift.
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

/// 320 distinct gradients in one frame — past the 255 rows the atlas
/// starts with, and every row is referenced by this frame's draws, so
/// none can be evicted. The atlas must grow, the backend must resize
/// its LUT texture, and the shaders must read the new height back
/// (`textureDimensions`) instead of a height baked in at pipeline
/// build.
///
/// Asserted per swatch against its own expected sRGB value rather than
/// by mutual distinctness: a permuted row assignment would satisfy
/// "all 320 differ" while painting every swatch wrong.
#[test]
fn overflowing_gradient_atlas_paints_every_swatch() {
    let img = render_swatches();
    for i in 0..SWATCHES {
        let want = swatch_color(i).to_srgba_u8();
        // Swatch centre — clear of the edge AA the composer leaves on
        // the quad boundary.
        let x = (i % COLS) * SWATCH + SWATCH / 2;
        let y = (i / COLS) * SWATCH + SWATCH / 2;
        let got = img.get_pixel(x, y).0;
        let delta = [
            got[0].abs_diff(want.r),
            got[1].abs_diff(want.g),
            got[2].abs_diff(want.b),
        ];
        // The LUT stores linear `f16`, which resolves finer than an sRGB
        // step, so only the encode's rounding is left; neighbouring
        // swatches are ≥5 sRGB units apart, so this fails on any row
        // mix-up.
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

/// Golden record of the same scene: pins the composed grid so a future
/// change to growth, row assignment, or LUT sampling shows up as a
/// visible diff rather than only as a per-pixel assertion.
#[test]
fn overflowing_gradient_atlas_matches_golden() {
    assert_matches_golden(GoldenName::OverflowingGradientAtlas, &render_swatches());
}

/// The LUT is sampled at texel centres: a ramp parameter `t` reads the
/// point `t · 255` between texel centres, the inverse of how the bake
/// placed them.
///
/// A hard stop from red to blue at 0.25 is stored at 64/255, so texel 64
/// is red and texel 65 blue, and the filter blends them over one texel.
/// Across a 1024 px rect, pixel 258 sits at `t = 258.5 / 1024`, which
/// reads `255 · t − 64 = 0.373` of the way to blue: red 0.627 and blue
/// 0.373 linear, sRGB 207 and 163. Sampled at `u = t`, it read
/// `256 · t − 64.5 = 0.125`: sRGB 240 and 99.
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

/// The showcase's gradients page itself, compiled from the example, so the
/// golden pins what the showcase draws rather than a copy of it.
#[path = "../../../examples/showcase/pages/gradients.rs"]
mod showcase_page;

/// The showcase's gradients page as one golden: every linear, radial and
/// conic tile, the spread modes and the interpolation spaces, through the
/// composer, the atlas bake, the shader sample and the blend.
#[test]
fn showcase_gradients_page_matches_golden() {
    // The column the showcase's shell gives a scrolling page.
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

/// Pin the linear-gradient paint path end-to-end: composer registers
/// the gradient with the LUT atlas, backend uploads the row, shader
/// samples the LUT in the brush-slot branch. A vertical (π/2 angle)
/// 2-stop gradient from a dark-navy to a brighter-blue gives a clear
/// luminance ramp that's eyeballable in the golden and catches both
/// the wiring and the shader sample position.
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

/// Pin: `Shape::rect(rect).fill(LinearGradient::builder(...))` lowered
/// through `Tree::add_shape` → `ShapeRecord::Rect { fill: Brush, .. }`
/// paints correctly. Slice-2 step 6 unblocks this — prior
/// to the widening, the lowering called `as_solid().expect(...)` and
/// panicked on any non-solid brush.
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

/// Pins the radial + conic shader paths end-to-end. Two side-by-side
/// frames: a centred radial (yellow core fading to navy) and a 4-stop
/// conic colour wheel. Mismatch flags drift in `eval_fill`'s radial /
/// conic branches, the atlas (stops, interpolation) keying, or the
/// `fill_axis` payload packing.
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
