use crate::primitives::math::domain::internals::assert_close;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::color_coords::ColorCoords;
use crate::primitives::paint::color::color_model::ColorModel;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;

/// Hue of each sRGB cube corner from the reference port; the test re-derives them.
const CORNER_HUES: [f32; 6] = [
    0.081_205_2,
    0.304_914_5,
    0.395_820_4,
    0.541_024_9,
    0.733_477_8,
    0.912_120_6,
];

const CORNERS: [RgbaF32; 6] = [
    RgbaF32::hex(0xff0000),
    RgbaF32::hex(0xffff00),
    RgbaF32::hex(0x00ff00),
    RgbaF32::hex(0x00ffff),
    RgbaF32::hex(0x0000ff),
    RgbaF32::hex(0xff00ff),
];

/// Index of pure blue in [`CORNERS`], the one corner off the Okhsv gamut edge.
const BLUE: usize = 4;

/// Each cube corner is a fully saturated, full-value Okhsv color of the hue above.
#[test]
fn cube_corners_are_the_gamut_edge() {
    for (idx, (corner, expected)) in CORNERS.iter().zip(CORNER_HUES).enumerate() {
        let coords = ColorCoords::from_color(*corner, ColorModel::Okhsv, 0.0);
        assert_close(
            coords.hue(),
            expected,
            2e-7,
            "the reference port's hues are printed to seven decimals, and \
             the f32 conversion adds up to two ulps",
        );
        // Platform cbrt/powf last-ulp differences grow through the gamut-edge search.
        let edge = "the platform's cbrt and powf move the gamut-edge search";
        assert_close(coords.saturation(), 1.0, 1e-5, edge);
        assert_close(coords.value(), 1.0, 1e-5, edge);

        if idx == BLUE {
            continue;
        }
        let back = okhsv(expected, 1.0, 1.0).to_color();
        let (got, want) = (back.to_srgba8(), corner.to_srgba8());
        assert_eq!(got, want, "corner {idx}");
    }
}

/// Pure blue is the one color the cube cannot name: Okhsv's gamut edge is the first crossing of the
/// chroma sweep, and pure blue sits in a later in-gamut island. The space is built this way; the port is not wrong.
#[test]
fn pure_blue_lies_outside_the_cube() {
    let edge = okhsv(CORNER_HUES[BLUE], 1.0, 1.0).to_color();
    assert_eq!(edge.to_srgba8(), SrgbaU8::hex(0x0037ff));
    // Reading pure blue back saturates both axes to within `f32` resolution (the side is platform-specific).
    // Eight ULPs of slack covers it and stays well inside a thousandth of an 8-bit step.
    let top = 1.0 - 4.0 * f32::EPSILON..=1.0;
    let coords = ColorCoords::from_color(RgbaF32::hex(0x0000ff), ColorModel::Okhsv, 0.0);
    assert!(
        top.contains(&coords.saturation()),
        "blue saturation {}",
        coords.saturation()
    );
    assert!(
        top.contains(&coords.value()),
        "blue value {}",
        coords.value()
    );
}

fn hue_gap(a: f32, b: f32) -> f32 {
    let raw = (a - b).abs();
    raw.min(1.0 - raw)
}

/// The forward map lands a hair outside the gamut at the red corner (-1/255); the clamp stops it reaching `RgbaF32`.
#[test]
fn the_gamut_edge_never_goes_negative() {
    for step in 0..360 {
        let c = okhsv(step as f32 / 360.0, 1.0, 1.0).to_color();
        assert!(
            c.r >= 0.0 && c.g >= 0.0 && c.b >= 0.0,
            "hue {step} produced {c:?}",
        );
        assert!(
            c.r <= 1.0 && c.g <= 1.0 && c.b <= 1.0,
            "hue {step} over one"
        );
    }
}

/// The value axis ends are absolute: black for every hue and saturation, white at zero saturation.
#[test]
fn the_value_ends_are_absolute() {
    let black = SrgbaU8::hex(0x000000);
    let white = SrgbaU8::hex(0xffffff);
    for step in 0..12 {
        let h = step as f32 / 12.0;
        assert_eq!(okhsv(h, 1.0, 0.0).to_color().to_srgba8(), black);
        assert_eq!(okhsv(h, 0.0, 0.0).to_color().to_srgba8(), black);
        assert_eq!(okhsv(h, 0.0, 1.0).to_color().to_srgba8(), white);
    }
}

/// Saturation moves chroma and leaves lightness alone; value moves lightness and leaves hue alone.
#[test]
fn the_axes_are_orthogonal_in_hue() {
    let hue = 0.7;
    for s in [0.25, 0.5, 0.75, 1.0] {
        for v in [0.3, 0.6, 1.0] {
            let back = ColorCoords::from_color(okhsv(hue, s, v).to_color(), ColorModel::Okhsv, hue);
            assert!(
                hue_gap(back.hue(), hue) < 2e-3,
                "s={s} v={v} moved the hue to {}",
                back.hue(),
            );
        }
    }
}

/// Okhsv coordinates at `(h, s, v)`.
const fn okhsv(h: f32, s: f32, v: f32) -> ColorCoords {
    ColorCoords::new(ColorModel::Okhsv, h, s, v)
}
