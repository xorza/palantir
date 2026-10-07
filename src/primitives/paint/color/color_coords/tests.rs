use crate::primitives::math::domain;
use crate::primitives::math::domain::internals::assert_close;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::color_coords::ColorCoords;
use crate::primitives::paint::color::color_model::ColorModel;
use crate::primitives::paint::color::hsv::Hsv;
use crate::primitives::paint::color::okhsv::Okhsv;

/// A model switch keeps the colour and moves only the axes.
#[test]
fn switching_model_keeps_the_colour() {
    for model in ColorModel::ALL {
        let start = ColorCoords::new(model, RgbaF32::hex(0x4cd3ff), 0.0);
        let other = start.with_model(match model {
            ColorModel::Okhsv => ColorModel::Hsv,
            ColorModel::Hsv => ColorModel::Okhsv,
        });
        assert_ne!(other.model(), start.model());
        let (got, want) = (
            other.to_color().to_srgba_u8(),
            start.to_color().to_srgba_u8(),
        );
        assert_eq!(got, want, "{model:?}");
    }
}

/// Grey has no hue in either model, so a switch and back carries the retained hue through.
#[test]
fn switching_model_keeps_greys_hue() {
    let mut coords = ColorCoords::new(ColorModel::Okhsv, RgbaF32::hex(0x808080), 0.0);
    coords.set_hue(0.42);
    let round_trip = coords
        .with_model(ColorModel::Hsv)
        .with_model(ColorModel::Okhsv);
    assert_eq!(round_trip.hue(), 0.42, "{}", round_trip.hue());
}

/// Switching to the model in use is the identity, axes included; re-deriving would move a grey's hue.
#[test]
fn switching_to_the_same_model_changes_nothing() {
    let mut coords = ColorCoords::new(ColorModel::Okhsv, RgbaF32::BLACK, 0.0);
    coords.set_hue(0.3);
    assert_eq!(coords.with_model(ColorModel::Okhsv), coords);
}

/// Every setter clamps, hue included: 1.0 stays 1.0 (red, as 0.0 is), 1.25 clamps to it.
#[test]
fn setters_clamp() {
    let mut coords = ColorCoords::default();
    coords.set_hue(1.0);
    assert_eq!(coords.hue(), 1.0);
    coords.set_hue(1.25);
    coords.set_saturation(2.0);
    coords.set_value(-1.0);
    assert_eq!(coords.hue(), 1.0);
    assert_eq!(coords.saturation(), 1.0);
    assert_eq!(coords.value(), 0.0);
    assert_eq!(
        coords.to_color().to_srgba_u8(),
        ColorCoords::default().to_color().to_srgba_u8(),
        "hue 1 is the colour hue 0 is",
    );

    // A non-finite axis is no value, so each setter reads it as 0.
    coords.set_hue(f32::NAN);
    coords.set_saturation(f32::INFINITY);
    coords.set_value(f32::NAN);
    assert_eq!(
        (coords.hue(), coords.saturation(), coords.value()),
        (0.0, 0.0, 0.0)
    );
}

/// Both models read raw axes alike on conversion (hue as a turn, rest as fractions), so out-of-range or non-finite axes paint their coerced value, never NaN: `1.25` wraps to `0.25`, `1.7` clamps to `1`, NaN or infinity reads `0`. Getters read alike, so fields and bars match the swatch.
#[test]
fn raw_axes_coerce_on_conversion() {
    let cases = [
        ((f32::NAN, 0.5, 0.5), (0.0, 0.5, 0.5)),
        ((0.25, f32::NAN, 0.5), (0.25, 0.0, 0.5)),
        ((0.25, 0.5, f32::INFINITY), (0.25, 0.5, 0.0)),
        ((1.25, 1.7, -0.3), (0.25, 1.0, 0.0)),
    ];
    for model in ColorModel::ALL {
        for ((h, s, v), (ch, cs, cv)) in cases {
            let got = axes(model, h, s, v).to_color();
            let want = axes(model, ch, cs, cv).to_color();
            assert_eq!(got, want, "{model:?} ({h}, {s}, {v})");
            assert!(domain::is_color(got), "{model:?}: {got:?}");
            let read = axes(model, h, s, v);
            assert_eq!(
                (read.hue(), read.saturation(), read.value()),
                (ch, cs, cv),
                "{model:?} ({h}, {s}, {v}): the getters read the painted axes",
            );
        }
    }
}

/// `model`'s coordinates at raw axes `(h, s, v)` through its own constructor (wraps and clamps), not the setters.
fn axes(model: ColorModel, h: f32, s: f32, v: f32) -> ColorCoords {
    match model {
        ColorModel::Okhsv => ColorCoords::Okhsv(Okhsv::new(h, s, v)),
        ColorModel::Hsv => ColorCoords::Hsv(Hsv::new(h, s, v)),
    }
}

/// Distance between two hues the short way round the circle.
fn hue_gap(a: f32, b: f32) -> f32 {
    let raw = (a - b).abs();
    raw.min(1.0 - raw)
}

/// What both models promise: every unit-cube triple is inside sRGB, so the round trip holds (9 × 8 × 8 samples, grey excluded, which keeps the fallback hue), and axes past their ends take what a drag gives them (hue wraps, the rest clamp).
#[test]
fn both_models_round_trip_keep_greys_hue_and_wrap_or_clamp() {
    for model in ColorModel::ALL {
        let mut worst = 0.0_f32;
        for hi in 0..9 {
            for si in 1..9 {
                for vi in 1..9 {
                    let (h, s, v) = (hi as f32 / 9.0, si as f32 / 8.0, vi as f32 / 8.0);
                    let back = ColorCoords::new(model, axes(model, h, s, v).to_color(), h);
                    worst = worst
                        .max(hue_gap(back.hue(), h))
                        .max((back.saturation() - s).abs())
                        .max((back.value() - v).abs());
                }
            }
        }
        assert_close(
            worst,
            0.0,
            1e-3,
            "the worst axis drift over the cube, through f32 conversions",
        );

        for level in [0.0, 0.25, 0.5, 1.0] {
            let grey = ColorCoords::new(model, RgbaF32::srgb(level, level, level), 0.618);
            assert_eq!(grey.hue(), 0.618, "{model:?}: grey at {level}");
            assert_close(
                grey.saturation(),
                0.0,
                1e-3,
                "grey's saturation, to the model's f32 rounding",
            );
        }

        let byte = |c: ColorCoords| c.to_color().to_srgba_u8();
        assert_eq!(
            byte(axes(model, 1.25, 2.0, 2.0)),
            byte(axes(model, 0.25, 1.0, 1.0)),
            "{model:?}: past the top",
        );
        assert_eq!(
            byte(axes(model, -0.75, -1.0, 0.5)),
            byte(axes(model, 0.25, 0.0, 0.5)),
            "{model:?}: past the bottom",
        );
    }
}
