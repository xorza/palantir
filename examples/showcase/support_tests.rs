//! The swatches, here and not in `support`, which the visual suite compiles whole, so these run
//! once, with the example.

use palantir::{ColorScheme, Okhsv, Palette, RgbaF32, SrgbaU8};

use crate::support::{Swatches, on_swatch_style};

/// Each scheme's swatches land on its luminance (dark 0.35, light 0.25), keep their hue (teal
/// 0.62, orange 0.18, lime 0.34, violet 0.87, red 0.09 turns) and their scheme's saturation (dark
/// 0.6, light 0.75), and so read at one contrast: 7.02:1 on the dark ground `#141414`
/// (`(0.35 + 0.05) / (0.006995 + 0.05)`) and 3.41:1 on the light `#fcfcfc`
/// (`(0.973445 + 0.05) / (0.25 + 0.05)`), over the 3:1 WCAG 1.4.11 asks; and 6.90:1 and 5.17:1
/// under the swatch ink `#14161a`, over the 4.5:1 of WCAG 1.4.3. The bytes pin the solve.
#[test]
fn the_swatches_hold_their_luminance_hue_and_floors() {
    let rgb = |r, g, b| SrgbaU8::rgb(r, g, b);
    for (scheme, luminance, saturation, ground_ratio, ink_ratio, bytes) in [
        (
            ColorScheme::Dark,
            0.35,
            0.6,
            7.0181,
            6.9000,
            [
                rgb(87, 170, 199),
                rgb(208, 147, 86),
                rgb(144, 169, 76),
                rgb(203, 130, 242),
                rgb(241, 127, 105),
            ],
        ),
        (
            ColorScheme::Light,
            0.25,
            0.75,
            3.4115,
            5.1750,
            [
                rgb(56, 147, 177),
                rgb(187, 123, 55),
                rgb(121, 146, 47),
                rgb(189, 95, 234),
                rgb(227, 95, 71),
            ],
        ),
    ] {
        let ground = Palette::for_scheme(scheme).window_bg;
        let ink = on_swatch_style().color;
        let swatches = Swatches::solve(scheme).all();
        for ((swatch, hue), want) in swatches
            .into_iter()
            .zip([0.62, 0.18, 0.34, 0.87, 0.09])
            .zip(bytes)
        {
            assert_eq!(swatch.to_srgba8(), want, "{scheme:?}");
            // The solve stops at a value at or over the luminance; 24 halvings leave it within
            // 2^-24 of the crossing, under ten f32 steps of luminance at these levels.
            let over = swatch.luminance() - luminance;
            assert!((0.0..3e-7).contains(&over), "{scheme:?} {swatch:?}: {over}");
            // Okhsv's round trip through linear RGB holds each axis to a few f32 steps.
            let axes = Okhsv::from_color(swatch, 0.0);
            assert!((axes.h - hue).abs() < 1e-6, "{scheme:?} hue {}", axes.h);
            assert!(
                (axes.s - saturation).abs() < 2e-6,
                "{scheme:?} sat {}",
                axes.s
            );
            // The ratios to four places.
            assert!((swatch.contrast_ratio(ground) - ground_ratio).abs() < 1e-4);
            assert!((ink.contrast_ratio(swatch) - ink_ratio).abs() < 1e-4);
        }
    }
    assert_ne!(
        Swatches::solve(ColorScheme::Dark),
        Swatches::solve(ColorScheme::Light),
        "each scheme solves its own set"
    );
    assert_eq!(on_swatch_style().color, RgbaF32::hex(0x14161a));
}
