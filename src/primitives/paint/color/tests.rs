use crate::internals::panic_probe;
use crate::primitives::paint::color::*;
use ron::ser;

/// Every sRGB byte survives `RgbaF32` exactly and `RgbaF16` (within half a display step), alpha included.
#[test]
fn srgb_bytes_survive_the_trip_through_every_wider_form() {
    for byte in 0u8..=255 {
        let srgb = SrgbaU8::new(byte, byte, byte, byte);
        assert_eq!(RgbaF32::from(srgb).to_srgba8(), srgb, "sRGB {byte} via f32");
        assert_eq!(
            RgbaF32::from(RgbaF32::from(srgb).to_f16()).to_srgba8(),
            srgb,
            "sRGB {byte} via f16"
        );
    }
}

/// Fails to compile if `RgbaF32::srgb`, `hex` or `hexa` stops being const. Both literals read
/// as the bytes they spell: `hexa` puts alpha last, and `hex` is `hexa` with alpha `ff`.
#[test]
fn rgb_is_const_constructible() {
    const _LITERAL: RgbaF32 = RgbaF32::srgb(0.2, 0.4, 0.8);
    const HEX: RgbaF32 = RgbaF32::hex(0x3366CC);
    const HEXA: RgbaF32 = RgbaF32::hexa(0x3366CC80);
    assert_eq!(HEXA.to_srgba8(), SrgbaU8::new(0x33, 0x66, 0xCC, 0x80));
    assert_eq!(HEX, RgbaF32::hexa(0x3366CCFF));
    assert_eq!(RgbaF32::hex(0xFF_FFFF), RgbaF32::WHITE, "the largest rgb");
    // An eight-digit literal in `hex` would lose its top byte and shift the rest.
    for wide in [0x0100_0000, 0x3366_CC80] {
        panic_probe::assert_panics_with("hex takes 0xRRGGBB", || RgbaF32::hex(wide));
    }
}

#[derive(Debug)]
struct RonRoundTrip {
    text: String,
    parsed: RgbaF32,
}

/// Through a file, and through `Display` and `FromStr`, which write and read the file's form.
fn ron_roundtrip(c: RgbaF32) -> RonRoundTrip {
    let text = ser::to_string(&c).expect("serialize");
    let parsed = ron::from_str(&text).expect("parse");
    assert_eq!(text, format!("\"{c}\""), "Display writes the file's form");
    assert_eq!(c.to_string().parse::<RgbaF32>().expect("FromStr"), parsed);
    RonRoundTrip { text, parsed }
}

#[test]
fn hex_round_trip_is_exact_over_all_bytes() {
    for byte in 0u8..=255 {
        let c = RgbaF32::from_srgba8(SrgbaU8::rgb(byte, byte, byte));
        let RonRoundTrip { text: s, parsed } = ron_roundtrip(c);
        assert_eq!(
            s,
            format!("\"#{byte:02x}{byte:02x}{byte:02x}\""),
            "byte {byte}"
        );
        assert_eq!(parsed, c, "byte {byte}");
    }
}

/// Alpha 1.0 emits 6 hex digits; any other alpha emits 8.
#[test]
fn opaque_emits_six_digits_translucent_emits_eight() {
    let s = ron_roundtrip(RgbaF32::srgb(0.2, 0.4, 0.8)).text;
    assert!(
        s.contains(r##""#3366cc""##),
        "opaque must emit 6 digits: {s}"
    );
    let s = ron_roundtrip(RgbaF32::from_srgba8(SrgbaU8::new(0x33, 0x66, 0xcc, 0x80))).text;
    assert!(
        s.contains(r##""#3366cc80""##),
        "translucent must emit 8 digits: {s}"
    );
}

#[test]
fn extremes_round_trip() {
    for (c, hex) in [
        (RgbaF32::TRANSPARENT, "#00000000"),
        (RgbaF32::WHITE, "#ffffff"),
        (RgbaF32::BLACK, "#000000"),
    ] {
        let RonRoundTrip { text: s, parsed } = ron_roundtrip(c);
        assert_eq!(s, format!("\"{hex}\""), "{c:?}");
        assert_eq!(parsed, c, "{c:?}");
    }
}

/// A color off the 8-bit grid is written as `color(srgb-linear …)` and loads back the same: an
/// alpha of 0.18 (45.9 255ths), linear channels between two bytes, which leave `/ a` out when
/// opaque, and the HDR and negative channels a color may hold.
#[test]
fn off_grid_colors_write_srgb_linear_and_load_back_the_same() {
    for (c, text) in [
        (
            RgbaF32::WHITE.with_alpha(0.18),
            "color(srgb-linear 1 1 1 / 0.18)",
        ),
        (
            RgbaF32::new(0.2, 0.4, 0.6, 1.0),
            "color(srgb-linear 0.2 0.4 0.6)",
        ),
        (
            RgbaF32::new(-0.5, 2.0, 0.0, 0.5),
            "color(srgb-linear -0.5 2 0 / 0.5)",
        ),
    ] {
        let RonRoundTrip { text: s, parsed } = ron_roundtrip(c);
        assert_eq!(s, format!("\"{text}\""), "{c:?}");
        assert_eq!(parsed, c, "{c:?}");
    }
    assert_eq!(
        ron_roundtrip(RgbaF32::WHITE.with_alpha8(46)).text,
        "\"#ffffff2e\"",
        "46 255ths is on the grid"
    );
}

/// The linear form takes any whitespace between its words and refuses another space, a missing
/// or extra channel, an empty alpha, a word run into `srgb-linear`, a missing `)`, and a
/// non-finite channel, under the color rule.
#[test]
fn the_linear_form_parses_its_words_and_refuses_the_rest() {
    assert_eq!(
        "color( srgb-linear  0.2 0.4\t0.6 /0.5 )".parse::<RgbaF32>(),
        Ok(RgbaF32::new(0.2, 0.4, 0.6, 0.5))
    );
    for bad in [
        "color(srgb 1 1 1)",
        "color(srgb-linear 1 1)",
        "color(srgb-linear 1 1 1 1)",
        "color(srgb-linear 1 1 1 / )",
        "color(srgb-linear 1 1 1 / 1 1)",
        "color(srgb-linear1 1 1)",
        "color(srgb-linear 1 1 1",
        "color(srgb-linear 1 x 1)",
    ] {
        assert_eq!(
            bad.parse::<RgbaF32>(),
            Err(ParseColorError::LinearForm),
            "{bad}"
        );
    }
    for bad in [
        "color(srgb-linear NaN 1 1)",
        "color(srgb-linear 1 1 1 / inf)",
        "color(srgb-linear 65520 1 1)",
    ] {
        assert_eq!(
            bad.parse::<RgbaF32>(),
            Err(ParseColorError::Channel),
            "{bad}"
        );
    }
}

#[test]
fn color_parse_accepts_with_and_without_hash() {
    assert_eq!(
        parse_hex("#3266cc").unwrap(),
        RgbaF32::from_srgba8(SrgbaU8::rgb(0x32, 0x66, 0xcc))
    );
    assert_eq!(
        parse_hex("3266cc").unwrap(),
        RgbaF32::from_srgba8(SrgbaU8::rgb(0x32, 0x66, 0xcc))
    );
    assert_eq!(
        parse_hex("#3266cc80").unwrap(),
        RgbaF32::from_srgba8(SrgbaU8::new(0x32, 0x66, 0xcc, 0x80))
    );
    assert_eq!(
        parse_hex("#3266CC").unwrap(),
        RgbaF32::from_srgba8(SrgbaU8::rgb(0x32, 0x66, 0xcc))
    );
    assert_eq!(
        parse_hex("#3266CC80").unwrap(),
        RgbaF32::from_srgba8(SrgbaU8::new(0x32, 0x66, 0xcc, 0x80))
    );
    assert_eq!("#3266cc80".parse::<RgbaF32>(), parse_hex("#3266cc80"));
    assert_eq!("#3266c".parse::<RgbaF32>(), Err(ParseColorError::Length));
}

/// A length outside six and eight digits is `Length` before any digit is read; a right length
/// with a non-hex byte is `Digit`. Each error names its form in `Display`.
#[test]
fn color_parse_rejects_malformed_input() {
    for (bad, want, why) in [
        ("", ParseColorError::Length, "empty"),
        ("#", ParseColorError::Length, "hash alone"),
        ("#abc", ParseColorError::Length, "3-digit not supported"),
        ("#abcde", ParseColorError::Length, "5-digit not supported"),
        ("#abcdefab12", ParseColorError::Length, "10-digit too long"),
        ("#zzzzzz", ParseColorError::Digit, "non-hex digits"),
        // Regression: the length arms select on bytes; slicing the `str` for "日本" (6 bytes) or
        // "αβγδ" (8) split a char boundary and panicked instead of rejecting.
        ("日本", ParseColorError::Digit, "6-byte non-ASCII"),
        ("#日本", ParseColorError::Digit, "6-byte non-ASCII, hashed"),
        ("αβγδ", ParseColorError::Digit, "8-byte non-ASCII"),
        // `u8::from_str_radix` accepts a leading sign, so `"+a+b+c"` would read as rgb(10, 11, 12).
        ("#+a+b+c", ParseColorError::Digit, "sign is not a hex digit"),
    ] {
        assert_eq!(parse_hex(bad), Err(want), "{why}");
    }
    assert_eq!(
        ParseColorError::Length.to_string(),
        "expected #rrggbb or #rrggbbaa"
    );
    assert_eq!(ParseColorError::Digit.to_string(), "invalid hex digit");
    assert_eq!(
        ParseColorError::LinearForm.to_string(),
        "expected color(srgb-linear r g b) or color(srgb-linear r g b / a)"
    );
    assert_eq!(ParseColorError::Channel.to_string(), domain::COLOR_RULE);
}

/// Each lane of `got` within `tol` of `want`, which comes from a double-precision Oklab reference.
#[track_caller]
fn assert_color_close(got: RgbaF32, want: [f64; 4], tol: f64, why: &str) {
    for (lane, (g, w)) in [got.r, got.g, got.b, got.a]
        .into_iter()
        .zip(want)
        .enumerate()
    {
        domain::internals::assert_close(g, w, tol, &format!("{why}, lane {lane}"));
    }
}

/// A trip through Oklab in `f32`: the cube root and the cube each round, and an in-range lane
/// lands at most 2.7e-7 off, about 2 ulps of 1, by these cases.
const OKLAB_TRIP: f64 = 5e-7;

/// Colors blend in Oklab premultiplied by alpha, the references from a double-precision Oklab:
///
/// - `a` fading to the transparent `b`: `b` is zero in motion space, so at a quarter the blend is
///   `a`'s coordinates at alpha 0.375, `a`'s own color. A straight blend would have dimmed it to
///   `(0.75, 0.25, 0.5)`.
/// - Black to white: a neutral's Oklab lightness is the cube root of its linear value, so the
///   midpoint `L = 0.5` is linear `0.125`, half way to the eye; linear light would give `0.5`.
/// - Red to green: the midpoint of their Oklab coordinates is linear `(0.6318, 0.3936, 0.0001)`.
///   `t = 2` overshoots green by the same delta; its largest cone root, 1.1632, passes white's 1,
///   so the cap scales the coordinates down to `(-1.4427, 1.9183, -0.0010)`.
/// - Past transparent the alpha goes negative and no color is left to recover.
///
/// The ends are exact, an equal pair returns itself with no round trip, and `t` is unclamped.
#[test]
fn lerp_blends_in_premultiplied_oklab_and_overshoots() {
    let a = RgbaF32::new(1.0, 0.0, 0.5, 0.5);
    let b = RgbaF32::new(0.0, 1.0, 0.5, 0.0);
    assert_eq!(Lerp::lerp(&a, &b, 0.0), a);
    assert_eq!(Lerp::lerp(&a, &b, 1.0), b);
    let teal = RgbaF32::srgb(0.1, 0.6, 0.7);
    assert_eq!(Lerp::lerp(&teal, &teal, 0.37), teal);
    assert_color_close(
        Lerp::lerp(&a, &b, 0.25),
        [1.0, 0.0, 0.5, 0.375],
        OKLAB_TRIP,
        "a fade keeps its hue",
    );
    assert_color_close(
        Lerp::lerp(&a, &b, 0.5),
        [1.0, 0.0, 0.5, 0.25],
        OKLAB_TRIP,
        "a fade keeps its hue",
    );

    assert_color_close(
        Lerp::lerp(&RgbaF32::BLACK, &RgbaF32::WHITE, 0.5),
        [0.125, 0.125, 0.125, 1.0],
        OKLAB_TRIP,
        "the perceptual midpoint of a neutral",
    );

    let (red, green) = (
        RgbaF32::new(1.0, 0.0, 0.0, 1.0),
        RgbaF32::new(0.0, 1.0, 0.0, 1.0),
    );
    assert_color_close(
        Lerp::lerp(&red, &green, 0.5),
        [0.631_818_39, 0.393_603_26, 0.000_136_99, 1.0],
        OKLAB_TRIP,
        "red to green",
    );
    assert_color_close(
        Lerp::lerp(&red, &green, 2.0),
        [-1.442_684_7, 1.918_345_5, -0.000_996_3, 1.0],
        4.0 * OKLAB_TRIP,
        "a capped overshoot, the trip's error scaled by the lanes' size",
    );
    assert_eq!(Lerp::lerp(&a, &b, 2.0), RgbaF32::new(0.0, 0.0, 0.0, -0.5));
}

/// A tween that overshoots toward a nearly transparent color stays bounded: opaque red to blue at
/// alpha 0.05, past its end, divides by an alpha near 0, and Oklab's cube would paint `1/α²`, a
/// blue of 50.6 at `t = 1.052`. The cap divides by the largest cone root instead, so the painted
/// color, the straight one times alpha, stays under the end's own 0.05; the references come from a
/// double-precision Oklab with the same cap. An HDR color is capped at white while it moves, its
/// ends exact; an equal pair does not move, so it keeps its value.
#[test]
fn an_overshoot_toward_a_nearly_transparent_color_stays_bounded() {
    let (red, faint_blue) = (
        RgbaF32::new(1.0, 0.0, 0.0, 1.0),
        RgbaF32::new(0.0, 0.0, 1.0, 0.05),
    );
    for (t, painted) in [
        (1.05, [-0.004_136_568, 0.000_574_980, 0.004_291_076]),
        (1.052, [-0.001_445_063, 0.000_268_546, 0.001_034_874]),
    ] {
        let got = Lerp::lerp(&red, &faint_blue, t);
        for (lane, (channel, want)) in [got.r, got.g, got.b].into_iter().zip(painted).enumerate() {
            domain::internals::assert_close(
                channel * got.a,
                want,
                1e-6,
                &format!(
                    "t = {t}, lane {lane}: the alpha near 0 is a difference of two lanes near 1, \
                     so it carries their f32 rounding"
                ),
            );
            assert!(
                channel * got.a <= 0.05,
                "t = {t}, lane {lane} under the end's blue"
            );
        }
    }

    // Oklab's L of a neutral is the cube root of its luminance, so at alpha ¾ halfway the
    // neutral's cone roots are 2^⅓ ≈ 1.26, over the alpha: the cap divides by them, to white, but
    // for the trip's rounding.
    let (hdr, faint_hdr) = (
        RgbaF32::new(2.0, 2.0, 2.0, 1.0),
        RgbaF32::new(2.0, 2.0, 2.0, 0.5),
    );
    assert_eq!(Lerp::lerp(&hdr, &faint_hdr, 1.0), faint_hdr);
    assert_eq!(
        Lerp::lerp(&hdr, &hdr, 0.5),
        hdr,
        "an equal pair does not move"
    );
    assert_color_close(
        Lerp::lerp(&hdr, &faint_hdr, 0.5),
        [0.999_999_94, 1.000_000_1, 0.999_999_34, 0.75],
        OKLAB_TRIP,
        "an HDR neutral capped at white",
    );
}

/// Motion space is Oklab premultiplied: `(1, 0, 0.5)` is Oklab `(0.66695, 0.27391, -0.07463)`, so
/// at alpha 0.5 it holds half of each. `from_motion` inverts it; with no alpha no color is left
/// to recover. The cube root keeps its sign, so a color outside the gamut whose long and medium
/// cone responses are negative, `(-0.5, 0, 0.1)`, goes there and back.
#[test]
fn motion_space_is_premultiplied_oklab() {
    let a = RgbaF32::new(1.0, 0.0, 0.5, 0.5);
    assert_color_close(
        a.to_motion(),
        [0.333_475_15, 0.136_952_73, -0.037_314_33, 0.5],
        OKLAB_TRIP,
        "half of the Oklab coordinates",
    );
    assert_color_close(
        RgbaF32::from_motion(a.to_motion()),
        [1.0, 0.0, 0.5, 0.5],
        OKLAB_TRIP,
        "there and back",
    );
    assert_eq!(
        RgbaF32::from_motion(RgbaF32::new(0.25, 0.5, 0.0, 0.0)),
        RgbaF32::new(0.0, 0.0, 0.0, 0.0)
    );
    let outside = RgbaF32::new(-0.5, 0.0, 0.1, 1.0);
    assert_color_close(
        RgbaF32::from_motion(outside.to_motion()),
        [-0.5, 0.0, 0.1, 1.0],
        OKLAB_TRIP,
        "out of gamut there and back",
    );
}

/// `faded` scales alpha only; color lanes stay bit-identical, and `by == 1.0` is the identity.
#[test]
fn faded_scales_only_the_alpha_lane() {
    let f16 = RgbaF16::new(0.25, 0.5, 0.75, 0.8);
    let full = RgbaF32::from(f16);
    let half = RgbaF32::from(f16.faded(0.5));
    assert_eq!((half.r, half.g, half.b), (full.r, full.g, full.b));
    assert_eq!(full.a, 1638.0 / 2048.0);
    assert_eq!(half.a, 1638.0 / 4096.0, "alpha is half of the packed 0.8");
    assert_eq!(f16.faded(1.0), f16);
    assert!(f16.faded(0.0).is_noop(), "a zero fade is fully transparent");
}

/// Lowering clamps alpha to `0..=1` and channels to `≥ 0`, and keeps HDR channels and NaN.
#[test]
fn lowering_clamps_what_a_tween_overshoots() {
    use crate::animation::easing::Easing;

    let lowered = |c: RgbaF32| RgbaF32::from(c.to_f16());
    assert_eq!(
        lowered(RgbaF32::new(-0.25, 2.0, 0.5, 1.5)),
        RgbaF32::new(0.0, 2.0, 0.5, 1.0),
    );
    assert_eq!(lowered(RgbaF32::new(0.5, 0.5, 0.5, -0.5)).a, 0.0);
    assert!(
        RgbaF16::new(f32::NAN, 0.0, 0.0, 1.0).is_noop(),
        "NaN survives"
    );
    assert_eq!(
        RgbaF16::new(0.25, 0.5, 0.75, 0.8).faded(2.0),
        RgbaF16::new(0.25, 0.5, 0.75, 1.0),
        "a fade past 1 clamps"
    );

    // `OutBack` overshoots its end by about 10 %: a fade in from transparent passes alpha 1.
    let t = (0..=100)
        .map(|i| i as f32 / 100.0)
        .max_by(|a, b| {
            Easing::OutBack
                .apply(*a)
                .total_cmp(&Easing::OutBack.apply(*b))
        })
        .unwrap();
    let peak = Lerp::lerp(
        &RgbaF32::TRANSPARENT,
        &RgbaF32::WHITE,
        Easing::OutBack.apply(t),
    );
    assert!(
        peak.a > 1.05,
        "premise: the tween overshoots, got {}",
        peak.a
    );
    assert_eq!(lowered(peak).a, 1.0);

    for a in [-0.5, -1e-3, f32::NEG_INFINITY] {
        assert!(RgbaF32::new(0.5, 0.5, 0.5, a).is_noop(), "{a}");
    }
}

/// A color the crate derives past 65504, as a tween overshoots, saturates where it lowers, the
/// other channels and the alpha kept: 65520 is the least channel that would pack to infinity.
#[test]
fn a_derived_color_past_65504_saturates_where_it_lowers() {
    let lowered = RgbaF32::from(RgbaF32::new(65520.0, 1.0e9, 0.5, 1.0).to_f16());
    assert_eq!(lowered, RgbaF32::new(65504.0, 65504.0, 0.5, 1.0));
}
