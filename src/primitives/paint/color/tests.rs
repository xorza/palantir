use crate::animation::animatable::Animatable;
use crate::primitives::paint::color::*;
use ron::ser;

/// Every sRGB byte survives `RgbaF32` exactly and `RgbaF16` (within half a display step), alpha included.
#[test]
fn srgb_bytes_survive_the_trip_through_every_wider_form() {
    for byte in 0u8..=255 {
        let srgb = SrgbaU8::new(byte, byte, byte, byte);
        assert_eq!(
            SrgbaU8::from(RgbaF32::from(srgb)),
            srgb,
            "sRGB {byte} via f32"
        );
        assert_eq!(
            SrgbaU8::from(RgbaF16::from(srgb)),
            srgb,
            "sRGB {byte} via f16"
        );
    }
}

/// Fails to compile if `RgbaF32::srgb` or `hex` stops being const.
#[test]
fn rgb_is_const_constructible() {
    const _LITERAL: RgbaF32 = RgbaF32::srgb(0.2, 0.4, 0.8);
    const _HEX: RgbaF32 = RgbaF32::hex(0x3366CC);
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
        let c = RgbaF32::from_srgba(SrgbaU8::rgb(byte, byte, byte));
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
    let s = ron_roundtrip(RgbaF32::srgba(0.2, 0.4, 0.8, 0.5)).text;
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

#[test]
fn color_parse_accepts_with_and_without_hash() {
    assert_eq!(
        parse_hex("#3266cc").unwrap(),
        RgbaF32::from_srgba(SrgbaU8::rgb(0x32, 0x66, 0xcc))
    );
    assert_eq!(
        parse_hex("3266cc").unwrap(),
        RgbaF32::from_srgba(SrgbaU8::rgb(0x32, 0x66, 0xcc))
    );
    assert_eq!(
        parse_hex("#3266cc80").unwrap(),
        RgbaF32::from_srgba(SrgbaU8::new(0x32, 0x66, 0xcc, 0x80))
    );
    assert_eq!(
        parse_hex("#3266CC").unwrap(),
        RgbaF32::from_srgba(SrgbaU8::rgb(0x32, 0x66, 0xcc))
    );
    assert_eq!(
        parse_hex("#3266CC80").unwrap(),
        RgbaF32::from_srgba(SrgbaU8::new(0x32, 0x66, 0xcc, 0x80))
    );
    assert_eq!("#3266cc80".parse::<RgbaF32>(), parse_hex("#3266cc80"));
    assert!("#3266c".parse::<RgbaF32>().is_err());
}

#[test]
fn color_parse_rejects_malformed_input() {
    assert!(parse_hex("").is_err());
    assert!(parse_hex("#").is_err());
    assert!(parse_hex("#abc").is_err(), "3-digit not supported");
    assert!(parse_hex("#abcde").is_err(), "5-digit not supported");
    assert!(parse_hex("#abcdefab12").is_err(), "10-digit too long");
    assert!(parse_hex("#zzzzzz").is_err(), "non-hex digits");
    // Regression: the length arms select on bytes; slicing the `str` for "日本" (6 bytes) or
    // "αβγδ" (8) split a char boundary and panicked instead of rejecting.
    assert!(parse_hex("日本").is_err(), "6-byte non-ASCII");
    assert!(parse_hex("#日本").is_err(), "6-byte non-ASCII, hashed");
    assert!(parse_hex("αβγδ").is_err(), "8-byte non-ASCII");
    // `u8::from_str_radix` accepts a leading sign, so `"+a+b+c"` would read as rgb(10, 11, 12).
    assert!(parse_hex("#+a+b+c").is_err(), "sign is not a hex digit");
}

#[test]
fn lerp_spans_both_endpoints_and_overshoots() {
    let a = RgbaF32::new(1.0, 0.0, 0.5, 0.5);
    let b = RgbaF32::new(0.0, 1.0, 0.5, 0.0);

    assert_eq!(Animatable::lerp(a, b, 0.0), a);
    assert_eq!(Animatable::lerp(a, b, 1.0), b);

    let quarter = Animatable::lerp(a, b, 0.25);
    assert_eq!(
        (quarter.r, quarter.g, quarter.b, quarter.a),
        (0.75, 0.25, 0.5, 0.375)
    );

    let mid = Animatable::lerp(a, b, 0.5);
    assert_eq!((mid.r, mid.g, mid.b, mid.a), (0.5, 0.5, 0.5, 0.25));

    // `t` is unclamped: t = 2 overshoots `b` by the same delta (r 1.0 → -1.0).
    assert_eq!(Animatable::lerp(a, b, 2.0).r, -1.0);
}

/// `faded` scales alpha only; colour lanes stay bit-identical, and `by == 1.0` is the identity.
#[test]
fn faded_scales_only_the_alpha_lane() {
    let f16 = RgbaF16::new(0.25, 0.5, 0.75, 0.8);
    let full = f16.unpack();
    let half = f16.faded(0.5).unpack();
    assert_eq!((half.r, half.g, half.b), (full.r, full.g, full.b));
    assert_eq!(full.a, 1638.0 / 2048.0);
    assert_eq!(half.a, 1638.0 / 4096.0, "alpha is half of the packed 0.8");
    assert_eq!(f16.faded(1.0), f16);
    assert!(f16.faded(0.0).is_noop(), "a zero fade is fully transparent");
}
