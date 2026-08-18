use crate::primitives::color::*;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

fn hash_value(value: impl Hash) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

/// Reference: spec-exact piecewise sRGB→linear (the previous in-tree
/// implementation). Used as ground truth for the cubic approximation.
fn srgb_to_linear_exact(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Pin: the cubic stays within ~1.5e-3 of the spec-exact piecewise
/// curve across `[0, 1]`. A regression past 2e-3 suggests the
/// coefficients drifted; revisit before shipping.
#[test]
fn cubic_srgb_max_error_under_two_thousandths() {
    let mut max_err: f32 = 0.0;
    // Sweep at 1/1024 resolution — finer than 8-bit display, plenty
    // to catch the worst-case point.
    for i in 0..=1024 {
        let c = i as f32 / 1024.0;
        let approx = srgb_to_linear(c);
        let exact = srgb_to_linear_exact(c);
        let err = (approx - exact).abs();
        if err > max_err {
            max_err = err;
        }
    }
    assert!(
        max_err < 2.0e-3,
        "cubic max error {max_err} exceeded 2e-3 threshold"
    );
}

/// Sanity: const-construction works in const context. If `Color::rgb`
/// regresses to non-const, this fails to compile.
#[test]
fn rgb_is_const_constructible() {
    const _LITERAL: Color = Color::rgb(0.2, 0.4, 0.8);
    const _HEX: Color = Color::hex(0x3366CC);
}

/// Roundtrip a Color through TOML and parse the emitted hex back.
/// Wraps in a tiny struct because TOML's top level must be a table.
fn toml_roundtrip(c: Color) -> (String, Color) {
    #[derive(serde::Serialize, serde::Deserialize)]
    struct W {
        c: Color,
    }
    let s = toml::to_string(&W { c }).expect("serialize");
    let parsed: W = toml::from_str(&s).expect("parse");
    (s, parsed.c)
}

/// Pin: serializing a Color and re-serializing the parse converges
/// to the same hex bytes for every (r, g, b) sRGB byte. Catches
/// Newton-iteration regressions that drift by 1 LSB.
#[test]
fn hex_round_trip_stable_over_all_bytes() {
    for byte in 0u8..=255 {
        let c = Color::rgb_u8(byte, byte, byte);
        let (s1, parsed) = toml_roundtrip(c);
        let (s2, _) = toml_roundtrip(parsed);
        assert_eq!(s1, s2, "byte {byte} did not round-trip stably");
    }
}

/// Pin: alpha = 1.0 emits the 6-digit form; any other alpha emits
/// the 8-digit form. A refactor that always emits 8 digits would
/// silently change the output format and trip this test.
#[test]
fn opaque_emits_six_digits_translucent_emits_eight() {
    // 0.2 → 0x33, 0.4 → 0x66, 0.8 → 0xcc once round-tripped through
    // the cubic / Newton inverse pair.
    let (s, _) = toml_roundtrip(Color::rgb(0.2, 0.4, 0.8));
    assert!(
        s.contains(r##""#3366cc""##),
        "opaque must emit 6 digits: {s}"
    );
    let (s, _) = toml_roundtrip(Color::rgba(0.2, 0.4, 0.8, 0.5));
    assert!(
        s.contains(r##""#3366cc80""##),
        "translucent must emit 8 digits: {s}"
    );
}

/// Edge cases: fully transparent, fully opaque white, opaque black.
#[test]
fn extremes_round_trip() {
    for c in [Color::TRANSPARENT, Color::WHITE, Color::BLACK] {
        let (s1, p) = toml_roundtrip(c);
        let (s2, _) = toml_roundtrip(p);
        assert_eq!(s1, s2);
    }
}

#[test]
fn color_parse_accepts_with_and_without_hash() {
    assert_eq!(
        parse_hex("#3266cc").unwrap(),
        Color::rgb_u8(0x32, 0x66, 0xcc)
    );
    assert_eq!(
        parse_hex("3266cc").unwrap(),
        Color::rgb_u8(0x32, 0x66, 0xcc)
    );
    assert_eq!(
        parse_hex("#3266cc80").unwrap(),
        Color::rgba_u8(0x32, 0x66, 0xcc, 0x80)
    );
    // Either digit case, both lengths. The serializer only ever
    // emits lowercase, so nothing else pins that a hand-written
    // uppercase theme file still parses.
    assert_eq!(
        parse_hex("#3266CC").unwrap(),
        Color::rgb_u8(0x32, 0x66, 0xcc)
    );
    assert_eq!(
        parse_hex("#3266CC80").unwrap(),
        Color::rgba_u8(0x32, 0x66, 0xcc, 0x80)
    );
}

#[test]
fn color_parse_rejects_malformed_input() {
    assert!(parse_hex("").is_err());
    assert!(parse_hex("#").is_err());
    assert!(parse_hex("#abc").is_err(), "3-digit not supported");
    assert!(parse_hex("#abcde").is_err(), "5-digit not supported");
    assert!(parse_hex("#abcdefab12").is_err(), "10-digit too long");
    assert!(parse_hex("#zzzzzz").is_err(), "non-hex digits");
    // Regression: the length arms select on bytes, so these reach a
    // digit decode rather than a `str` index. `"日本"` is 6 bytes and
    // `"αβγδ"` is 8, hitting both arms; indexing the `str` split a
    // char boundary and panicked instead of rejecting the input —
    // in a deserializer whose whole job is to reject it.
    assert!(parse_hex("日本").is_err(), "6-byte non-ASCII");
    assert!(parse_hex("#日本").is_err(), "6-byte non-ASCII, hashed");
    assert!(parse_hex("αβγδ").is_err(), "8-byte non-ASCII");
    // `u8::from_str_radix` accepts a leading sign, so a parser that
    // delegates to it reads `"+a+b+c"` as rgb(10, 11, 12).
    assert!(parse_hex("#+a+b+c").is_err(), "sign is not a hex digit");
}

#[test]
fn equal_signed_zero_colors_have_equal_hashes() {
    let positive = Color::linear_rgba(0.0, 0.0, 0.0, 0.0);
    let negative = Color::linear_rgba(-0.0, -0.0, -0.0, -0.0);

    assert_eq!(positive, negative);
    assert_eq!(hash_value(positive), hash_value(negative));
}

#[test]
fn lerp_spans_both_endpoints_and_agrees_with_midpoint() {
    // Every channel here is an exact binary fraction, so the arithmetic
    // below is checkable by eye and by `==`.
    let a = Color::linear_rgba(1.0, 0.0, 0.5, 0.5);
    let b = Color::linear_rgba(0.0, 1.0, 0.5, 0.0);

    // The endpoints come back exactly, alpha included.
    assert_eq!(a.lerp(b, 0.0), a);
    assert_eq!(a.lerp(b, 1.0), b);

    // Hand-computed quarter step: r 1.0→0.75, g 0.0→0.25, b flat at 0.5,
    // a 0.5→0.375. Alpha travels with the color, unlike `with_alpha`.
    let quarter = a.lerp(b, 0.25);
    assert_eq!(
        (quarter.r, quarter.g, quarter.b, quarter.a),
        (0.75, 0.25, 0.5, 0.375)
    );

    // `midpoint` is the symmetric spelling of the same blend.
    let mid = a.lerp(b, 0.5);
    let sym = a.midpoint(b);
    for (l, r) in [
        (mid.r, sym.r),
        (mid.g, sym.g),
        (mid.b, sym.b),
        (mid.a, sym.a),
    ] {
        assert!((l - r).abs() < 1e-6, "lerp(0.5) = {l}, midpoint = {r}");
    }

    // `t` is deliberately unclamped, so a caller can overshoot: t = 2
    // continues past `b` by the same delta again (r 1.0 → -1.0).
    assert_eq!(a.lerp(b, 2.0).r, -1.0);
}

/// The direct `ColorF16 → ColorU8` quantize must stay byte-identical
/// to the two-hop form (`ColorU8::from(Color::from(x))`) it replaced
/// at the composer's per-run/tint call sites.
#[test]
fn f16_to_u8_matches_two_hop_quantize() {
    for c in [
        Color::linear_rgba(0.0, 0.0, 0.0, 0.0),
        Color::linear_rgba(1.0, 0.25, 0.5, 1.0),
        Color::linear_rgba(0.1, 0.9, 0.33, 0.5),
        Color::linear_rgba(1.0, 1.0, 1.0, 1.0),
    ] {
        let f16 = ColorF16::from(c);
        assert_eq!(ColorU8::from(f16), ColorU8::from(Color::from(f16)));
    }
}
