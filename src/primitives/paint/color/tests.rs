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

fn ron_roundtrip(c: RgbaF32) -> RonRoundTrip {
    let text = ser::to_string(&c).expect("serialize");
    let parsed = ron::from_str(&text).expect("parse");
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

/// Colors blend premultiplied: `a` premultiplies to `(0.5, 0, 0.25 | 0.5)` and the transparent `b`
/// to zero, so a quarter of the way the blend is `(0.375, 0, 0.1875 | 0.375)`, which is `a`'s hue
/// at alpha 0.375. A straight blend would have dimmed it to `(0.75, 0.25, 0.5)`. The ends are
/// exact, and `t` is unclamped.
#[test]
fn lerp_blends_premultiplied_and_overshoots() {
    let a = RgbaF32::new(1.0, 0.0, 0.5, 0.5);
    let b = RgbaF32::new(0.0, 1.0, 0.5, 0.0);

    assert_eq!(Animatable::lerp(a, b, 0.0), a);
    assert_eq!(Animatable::lerp(a, b, 1.0), b);
    assert_eq!(
        Animatable::lerp(a, b, 0.25),
        RgbaF32::new(1.0, 0.0, 0.5, 0.375)
    );
    assert_eq!(
        Animatable::lerp(a, b, 0.5),
        RgbaF32::new(1.0, 0.0, 0.5, 0.25)
    );
    // Between two opaque colors the blend is the plain one: 0.5 of the way from red to green.
    let (red, green) = (
        RgbaF32::new(1.0, 0.0, 0.0, 1.0),
        RgbaF32::new(0.0, 1.0, 0.0, 1.0),
    );
    assert_eq!(
        Animatable::lerp(red, green, 0.5),
        RgbaF32::new(0.5, 0.5, 0.0, 1.0)
    );
    // `t = 2` overshoots `green` by the same delta: r 1.0 → -1.0.
    assert_eq!(Animatable::lerp(red, green, 2.0).r, -1.0);
    // Past transparent the alpha goes negative and no color is left to recover.
    assert_eq!(
        Animatable::lerp(a, b, 2.0),
        RgbaF32::new(0.0, 0.0, 0.0, -0.5)
    );
}

/// Motion space is premultiplied: `to_motion` premultiplies, `from_motion` inverts it, and with
/// no alpha no color is left to recover.
#[test]
fn motion_space_is_premultiplied() {
    let a = RgbaF32::new(1.0, 0.0, 0.5, 0.5);
    assert_eq!(a.to_motion(), RgbaF32::new(0.5, 0.0, 0.25, 0.5));
    assert_eq!(RgbaF32::from_motion(a.to_motion()), a);
    assert_eq!(
        RgbaF32::from_motion(RgbaF32::new(0.25, 0.5, 0.0, 0.0)),
        RgbaF32::new(0.0, 0.0, 0.0, 0.0)
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
    let peak = Animatable::lerp(
        RgbaF32::TRANSPARENT,
        RgbaF32::WHITE,
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
