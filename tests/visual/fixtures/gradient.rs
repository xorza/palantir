//! Gradient LUT atlas fixtures — the end-to-end proof that a frame
//! authoring more distinct gradients than the atlas holds still paints
//! every one of them correctly.

use aperture::{Color, ColorU8, Configure, LinearGradient, Panel, Rect, Shape, Sizing};
use glam::UVec2;
use image::RgbaImage;

use crate::diff::Tolerance;
use crate::golden::assert_matches_golden;
use crate::harness::Harness;

/// More distinct gradients than the atlas's 256 initial rows (255
/// usable), so one frame forces it to grow. 20 × 16 swatches.
const COLS: u32 = 20;
const ROWS: u32 = 16;
const SWATCHES: u32 = COLS * ROWS;
const SWATCH: u32 = 8;
const VIEWPORT: UVec2 = UVec2::new(COLS * SWATCH, ROWS * SWATCH);
const CLEAR: Color = Color::BLACK;

/// Linear-u8 stop colour for swatch `i`. Channels are spread far
/// enough apart that neighbouring swatches stay distinguishable after
/// the sRGB framebuffer encode, so sampling the wrong LUT row can't
/// pass as rounding.
fn swatch_color(i: u32) -> ColorU8 {
    ColorU8::rgb(
        (40 + (i % COLS) * 10) as u8,
        (40 + (i / COLS) * 12) as u8,
        200,
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
    harness.render(VIEWPORT, 1.0, CLEAR, |ui| {
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
        let want = Color::from(swatch_color(i)).to_srgb_u8();
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
        // ±3 covers the f16 LUT store plus the linear→sRGB encode
        // rounding; neighbouring swatches are ≥5 sRGB units apart, so
        // this still fails on any row mix-up.
        assert!(
            delta.iter().all(|&d| d <= 3),
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
    assert_matches_golden(
        "overflowing_gradient_atlas",
        &render_swatches(),
        Tolerance::default(),
    );
}
