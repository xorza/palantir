use crate::primitives::color::RgbaF32;
use crate::primitives::color::okhsv::Okhsv;

/// Hue of each sRGB cube corner, measured with the reference port. The test
/// below re-derives them, so a drift in the conversion cannot hide behind a
/// stale constant.
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

fn srgb(c: RgbaF32) -> [u8; 3] {
    let q = c.to_srgba_u8();
    [q.r, q.g, q.b]
}

/// Index of pure blue in [`CORNERS`] — the one corner that is not on the
/// Okhsv gamut edge. See `pure_blue_lies_outside_the_cube`.
const BLUE: usize = 4;

/// Each cube corner is a fully saturated, full-value Okhsv colour, and its
/// hue is the constant above.
#[test]
fn cube_corners_are_the_gamut_edge() {
    for (index, (corner, expected)) in CORNERS.iter().zip(CORNER_HUES).enumerate() {
        let coords = Okhsv::from_color(*corner, 0.0);
        assert!(
            (coords.h - expected).abs() < 1e-4,
            "hue of {:?}: {} vs {expected}",
            srgb(*corner),
            coords.h,
        );
        assert!(coords.s > 0.999, "corner saturation {}", coords.s);
        assert!(coords.v > 0.999, "corner value {}", coords.v);

        if index == BLUE {
            continue;
        }
        let back = Okhsv::new(expected, 1.0, 1.0).to_color();
        let (got, want) = (srgb(back), srgb(*corner));
        for channel in 0..3 {
            let delta = i16::from(got[channel]) - i16::from(want[channel]);
            assert!(delta.abs() <= 1, "corner {want:?} came back {got:?}");
        }
    }
}

/// Pure blue is the one colour the cube cannot name, and the numbers here say
/// by how much.
///
/// Sweeping chroma out along blue's hue, red dips below zero at `C/L ≈ 0.588`
/// and returns at `≈ 0.69`, and green only leaves at `≈ 0.695`. Pure blue
/// sits at `0.693`, inside that second island. Okhsv's edge is the first
/// crossing, so the island is outside it. The space is built this way; the
/// port is not wrong.
#[test]
fn pure_blue_lies_outside_the_cube() {
    let edge = Okhsv::new(CORNER_HUES[BLUE], 1.0, 1.0).to_color();
    assert_eq!(srgb(edge), [0, 56, 255]);
    // Reading pure blue back saturates both axes rather than reporting
    // something out of range, so a picker opened on it shows its handles in
    // the corner.
    let coords = Okhsv::from_color(RgbaF32::hex(0x0000ff), 0.0);
    assert_eq!(coords.s, 1.0);
    assert_eq!(coords.v, 1.0);
}

/// Distance between two hues the short way round the circle.
fn hue_gap(a: f32, b: f32) -> f32 {
    let raw = (a - b).abs();
    raw.min(1.0 - raw)
}

/// Every triple in the unit cube is inside sRGB, so nothing clamps away and
/// the round trip holds. 9³ samples of the cube, grey excluded: it has no hue
/// to recover and the fallback answers for it instead.
#[test]
fn round_trip_holds_across_the_cube() {
    let mut worst = 0.0_f32;
    for hi in 0..9 {
        for si in 1..9 {
            for vi in 1..9 {
                let start = Okhsv::new(hi as f32 / 9.0, si as f32 / 8.0, vi as f32 / 8.0);
                let back = Okhsv::from_color(start.to_color(), start.h);
                worst = worst
                    .max(hue_gap(back.h, start.h))
                    .max((back.s - start.s).abs())
                    .max((back.v - start.v).abs());
            }
        }
    }
    assert!(worst < 1e-3, "worst axis drift {worst}");
}

/// The forward map lands a hair outside the gamut at the red corner — the
/// reference returns -1/255 there. Without the clamp that reaches `RgbaF32`.
#[test]
fn the_gamut_edge_never_goes_negative() {
    for step in 0..360 {
        let c = Okhsv::new(step as f32 / 360.0, 1.0, 1.0).to_color();
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

/// Grey has no hue to recover, so the caller's fallback answers. This is what
/// stops a picker losing its hue at the bottom of the field.
#[test]
fn grey_keeps_the_fallback_hue() {
    for level in [0.0, 0.25, 0.5, 1.0] {
        let grey = RgbaF32::srgb(level, level, level);
        let coords = Okhsv::from_color(grey, 0.6180);
        assert_eq!(coords.h, 0.6180, "grey at {level} lost the fallback");
        assert!(
            coords.s < 1e-3,
            "grey at {level} has saturation {}",
            coords.s
        );
    }
}

/// The two ends of the value axis are absolute: black for every hue and
/// saturation, white for every hue at zero saturation.
#[test]
fn the_value_ends_are_absolute() {
    for step in 0..12 {
        let h = step as f32 / 12.0;
        assert_eq!(srgb(Okhsv::new(h, 1.0, 0.0).to_color()), [0, 0, 0]);
        assert_eq!(srgb(Okhsv::new(h, 0.0, 0.0).to_color()), [0, 0, 0]);
        assert_eq!(srgb(Okhsv::new(h, 0.0, 1.0).to_color()), [255, 255, 255]);
    }
}

/// A picker drives the axes past their ends every drag. Both directions take
/// it: the hue wraps, the other two clamp.
#[test]
fn out_of_range_axes_wrap_and_clamp() {
    assert_eq!(
        srgb(Okhsv::new(1.25, 2.0, 2.0).to_color()),
        srgb(Okhsv::new(0.25, 1.0, 1.0).to_color()),
    );
    assert_eq!(
        srgb(Okhsv::new(-0.75, -1.0, 0.5).to_color()),
        srgb(Okhsv::new(0.25, 0.0, 0.5).to_color()),
    );
}

/// Saturation moves chroma and leaves lightness alone; value moves lightness
/// and leaves hue alone. This is the whole reason the space is here, so it is
/// pinned rather than assumed.
#[test]
fn the_axes_are_orthogonal_in_hue() {
    let hue = 0.7;
    for s in [0.25, 0.5, 0.75, 1.0] {
        for v in [0.3, 0.6, 1.0] {
            let back = Okhsv::from_color(Okhsv::new(hue, s, v).to_color(), hue);
            assert!(
                hue_gap(back.h, hue) < 2e-3,
                "s={s} v={v} moved the hue to {}",
                back.h,
            );
        }
    }
}
