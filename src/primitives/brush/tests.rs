use crate::animation::animatable::Animatable;
use crate::primitives::brush::{
    Brush, ConicGradient, GradientStops, Interp, LinearGradient, MAX_STOPS, RadialGradient, Spread,
    Stop,
};
use crate::primitives::color::{Color, ColorU8};
use glam::Vec2;
use std::collections::hash_map::DefaultHasher;
use std::f32::consts::{FRAC_PI_4, PI};
use std::fmt::Write as _;
use std::hash::{Hash, Hasher};
use tinyvec::ArrayVec;

fn h(b: Brush) -> u64 {
    let mut s = DefaultHasher::new();
    b.hash(&mut s);
    s.finish()
}

#[derive(Debug, ::serde::Deserialize)]
struct StopsDocument {
    stops: GradientStops,
}

/// `LinearGradient::Hash` feeds `GradientCpuAtlas::register`'s
/// content-hashed row addressing — `±0.0` and NaN bit-pattern variants
/// must collapse so visually-identical gradients reuse one atlas row.
#[test]
fn linear_gradient_canon_bits_collapses_equivalent_f32_patterns() {
    let nan_a = f32::from_bits(0x7fc0_0001);
    let nan_b = f32::from_bits(0x7fc0_0002);
    assert!(nan_a.is_nan() && nan_b.is_nan());
    let cases: &[(&str, Brush, Brush)] = &[
        (
            "angle_neg_zero_eq_pos_zero",
            Brush::Linear(LinearGradient::two_stop(0.0, Color::BLACK, Color::WHITE)),
            Brush::Linear(LinearGradient::two_stop(-0.0, Color::BLACK, Color::WHITE)),
        ),
        (
            "angle_nan_bit_patterns_collapse",
            Brush::Linear(LinearGradient::two_stop(nan_a, Color::BLACK, Color::WHITE)),
            Brush::Linear(LinearGradient::two_stop(nan_b, Color::BLACK, Color::WHITE)),
        ),
        (
            "stop_offset_neg_zero_eq_pos_zero",
            Brush::Linear(LinearGradient::new(
                0.0,
                [Stop::new(0.0, Color::BLACK), Stop::new(1.0, Color::WHITE)],
            )),
            Brush::Linear(LinearGradient::new(
                0.0,
                [Stop::new(-0.0, Color::BLACK), Stop::new(1.0, Color::WHITE)],
            )),
        ),
    ];
    for (label, x, y) in cases {
        assert_eq!(h(x.clone()), h(y.clone()), "case: {label}");
    }
}

#[test]
fn from_color_round_trip() {
    let c = Color::WHITE;
    let b: Brush = c.into();
    assert_eq!(b, Brush::Solid(c));
    assert_eq!(b.as_solid(), Some(c));
}

#[test]
fn solid_solid_animatable_lerp_matches_color() {
    let a = Color::BLACK;
    let b = Color::WHITE;
    let mid_color = Color::lerp(a, b, 0.5);
    let mid_brush = Brush::lerp(Brush::Solid(a), Brush::Solid(b), 0.5);
    assert_eq!(mid_brush, Brush::Solid(mid_color));
}

#[test]
fn solid_is_noop_iff_color_is_noop() {
    assert!(Brush::Solid(Color::TRANSPARENT).is_noop());
    assert!(!Brush::Solid(Color::BLACK).is_noop());
}

/// `LinearGradient` is inline-stored on every `Brush::Linear`, so
/// its size sets the floor for `Brush`, `Background.fill`,
/// `Stroke.brush`, and every `Shape::*` variant carrying a brush.
/// Pin the size so any silent footprint regression (added field,
/// stop-cap bump) trips a test rather than diffusing through the
/// codebase. The exact number below is a function of `MAX_STOPS = 8`
/// + `repr(C)` field layout; recompute when those change.
#[test]
fn linear_gradient_size_is_compact() {
    // 4 (angle) + ArrayVec<[Stop; 8]> with Stop = 5 B (1 offset_u8 + 4 ColorU8)
    // + 1 (spread) + 1 (interp) + tail-pad. Recompute if MAX_STOPS or
    // Stop layout changes. Pinned to catch unintended layout drift.
    assert_eq!(std::mem::size_of::<LinearGradient>(), 48);
    assert_eq!(
        std::mem::size_of::<GradientStops>(),
        std::mem::size_of::<ArrayVec<[Stop; MAX_STOPS]>>(),
        "the validated wrapper must retain inline ArrayVec storage",
    );
}

#[test]
fn gradient_stop_count_is_enforced_by_construction_and_deserialization() {
    let stops = |count: usize| {
        (0..count)
            .map(|index| Stop::new(index as f32 / count.max(1) as f32, ColorU8::WHITE))
            .collect::<Vec<_>>()
    };
    let serialized = |count: usize| {
        if count == 0 {
            return "stops = []\n".to_owned();
        }
        let mut document = String::new();
        for index in 0..count {
            let offset = index as f32 / count.max(1) as f32;
            writeln!(
                document,
                "[[stops]]\noffset = {offset}\ncolor = {{ r = 255, g = 255, b = 255, a = 255 }}"
            )
            .unwrap();
        }
        document
    };

    for count in [0, 1, 2, 8, 9] {
        let constructed =
            std::panic::catch_unwind(|| GradientStops::new(stops(count))).map(|value| value.len());
        let deserialized =
            toml::from_str::<StopsDocument>(&serialized(count)).map(|value| value.stops.len());
        let expected = (2..=MAX_STOPS).contains(&count);
        assert_eq!(constructed.is_ok(), expected, "constructor count {count}");
        assert_eq!(deserialized.is_ok(), expected, "deserializer count {count}",);
        if expected {
            assert_eq!(constructed.unwrap(), count);
            assert_eq!(deserialized.unwrap(), count);
        }
    }
}

#[test]
fn non_finite_stop_offsets_are_rejected_at_both_boundaries() {
    for (label, offset) in [
        ("nan", f32::NAN),
        ("positive infinity", f32::INFINITY),
        ("negative infinity", f32::NEG_INFINITY),
    ] {
        assert!(
            std::panic::catch_unwind(|| Stop::new(offset, ColorU8::WHITE)).is_err(),
            "{label} must panic at the authoring boundary",
        );
    }

    for literal in ["nan", "inf", "-inf"] {
        let document = format!(
            "[[stops]]\noffset = {literal}\ncolor = {{ r = 255, g = 255, b = 255, a = 255 }}\n\
             [[stops]]\noffset = 1.0\ncolor = {{ r = 0, g = 0, b = 0, a = 255 }}\n"
        );
        let error = toml::from_str::<StopsDocument>(&document).unwrap_err();
        assert!(
            error.to_string().contains("offset must be finite"),
            "{literal} produced unexpected error: {error}",
        );
    }
}

#[test]
fn every_gradient_variant_round_trips_validated_stops() {
    #[derive(Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
    struct BrushDocument {
        brush: Brush,
    }

    let brushes = [
        Brush::Linear(LinearGradient::two_stop(
            0.25,
            ColorU8::BLACK,
            ColorU8::WHITE,
        )),
        Brush::Radial(RadialGradient::two_stop_centered(
            ColorU8::BLACK,
            ColorU8::WHITE,
        )),
        Brush::Conic(ConicGradient::two_stop_centered(
            ColorU8::BLACK,
            ColorU8::WHITE,
        )),
    ];
    for brush in brushes {
        let document = BrushDocument { brush };
        let encoded = toml::to_string(&document).expect("serialize valid gradient");
        let decoded =
            toml::from_str::<BrushDocument>(&encoded).expect("deserialize valid gradient");
        assert_eq!(decoded, document);
    }
}

#[test]
fn linear_two_stop_authoring() {
    let g = LinearGradient::two_stop(0.0, Color::hex(0x1a1a2e), Color::hex(0x16213e));
    assert_eq!(g.stops.len(), 2);
    assert_eq!(g.stops[0].offset(), 0.0);
    assert_eq!(g.stops[1].offset(), 1.0);
    assert_eq!(g.spread, Spread::Pad);
    assert_eq!(g.interp, Interp::Oklab);
    assert!(!g.is_noop());

    let overridden = g
        .clone()
        .with_spread(Spread::Repeat)
        .with_interp(Interp::Linear);
    assert_eq!(overridden.spread, Spread::Repeat);
    assert_eq!(overridden.interp, Interp::Linear);
    assert_eq!(overridden.stops, g.stops);
    assert_eq!(overridden.angle, g.angle);
}

#[test]
fn linear_three_stop_authoring() {
    let g = LinearGradient::three_stop(
        PI / 2.0,
        Color::hex(0x000000),
        Color::hex(0x808080),
        Color::hex(0xffffff),
    );
    assert_eq!(g.stops.len(), 3);
    assert!((g.stops[1].offset() - 0.5).abs() < 1.0 / 255.0);
}

#[test]
fn linear_all_transparent_is_noop() {
    let g = LinearGradient::two_stop(0.0, ColorU8::TRANSPARENT, ColorU8::rgba(255, 255, 255, 0));
    assert!(g.is_noop());
    assert!(Brush::Linear(g).is_noop());
}

#[test]
#[should_panic(expected = "exceeds MAX_STOPS")]
fn linear_too_many_stops_panics() {
    let many: Vec<Stop> = (0..=MAX_STOPS)
        .map(|i| Stop::new(i as f32 / 8.0, Color::WHITE))
        .collect();
    let _ = LinearGradient::new(0.0, many);
}

#[test]
#[should_panic(expected = "at least 2 stops")]
fn linear_one_stop_panics() {
    let _ = LinearGradient::new(0.0, [Stop::new(0.0, Color::WHITE)]);
}

#[test]
fn linear_brush_animatable_snaps_on_t_one() {
    let g0 = LinearGradient::two_stop(0.0, Color::BLACK, Color::WHITE);
    let g1 = LinearGradient::two_stop(0.0, Color::WHITE, Color::BLACK);
    let a = Brush::Linear(g0);
    let b = Brush::Linear(g1);
    assert_eq!(Brush::lerp(a.clone(), b.clone(), 0.5), a);
    assert_eq!(Brush::lerp(a, b.clone(), 1.0), b);
}

fn assert_spring_normalizes_to_target(mut current: Brush, target: Brush) {
    let mut velocity = Brush::Solid(Color::rgba(0.25, -0.5, 0.75, 1.0));
    current.normalize_for_spring(&target, &mut velocity);
    assert_eq!(current, target);
    assert_eq!(velocity, Brush::TRANSPARENT);
}

#[test]
fn gradient_brush_spring_normalization_is_direction_independent() {
    let solid = Brush::Solid(Color::hex(0x336699));
    let gradients = [
        Brush::Linear(LinearGradient::two_stop(0.25, Color::BLACK, Color::WHITE)),
        Brush::Radial(RadialGradient::two_stop_centered(
            Color::BLACK,
            Color::WHITE,
        )),
        Brush::Conic(ConicGradient::two_stop_centered(Color::BLACK, Color::WHITE)),
    ];
    let replacement_gradients = [
        Brush::Linear(LinearGradient::two_stop(0.75, Color::WHITE, Color::BLACK)),
        Brush::Radial(RadialGradient::two_stop_centered(
            Color::WHITE,
            Color::BLACK,
        )),
        Brush::Conic(ConicGradient::two_stop_centered(Color::WHITE, Color::BLACK)),
    ];

    for gradient in &gradients {
        assert_spring_normalizes_to_target(solid.clone(), gradient.clone());
        assert_spring_normalizes_to_target(gradient.clone(), solid.clone());
    }
    for source in &gradients {
        for target in &replacement_gradients {
            assert_spring_normalizes_to_target(source.clone(), target.clone());
        }
    }
}

#[test]
fn radial_default_centered() {
    let g = RadialGradient::two_stop_centered(Color::WHITE, Color::BLACK);
    assert_eq!(g.center, Vec2::splat(0.5));
    assert_eq!(g.radius, Vec2::splat(0.5));
    assert_eq!(g.interp, Interp::Oklab);
    assert_eq!(g.spread, Spread::Pad);
    let a = g.axis();
    assert_eq!(a.lanes(), [0.5, 0.5, 0.5, 0.5]);
}

#[test]
fn conic_default_linear_interp_per_variant() {
    let g = ConicGradient::two_stop_centered(Color::rgb(1.0, 0.0, 0.0), Color::rgb(0.0, 0.0, 1.0));
    assert_eq!(g.interp, Interp::Linear);
    let l = LinearGradient::two_stop(0.0, Color::rgb(1.0, 0.0, 0.0), Color::rgb(0.0, 0.0, 1.0));
    assert_eq!(l.interp, Interp::Oklab);
    let r = RadialGradient::two_stop_centered(Color::rgb(1.0, 0.0, 0.0), Color::rgb(0.0, 0.0, 1.0));
    assert_eq!(r.interp, Interp::Oklab);
}

#[test]
fn conic_axis_packs_start_angle() {
    let g = ConicGradient::new(
        Vec2::new(0.4, 0.6),
        FRAC_PI_4,
        [
            Stop::new(0.0, Color::rgb(1.0, 0.0, 0.0)),
            Stop::new(1.0, Color::rgb(0.0, 0.0, 1.0)),
        ],
    );
    let [dx, dy, t0, _] = g.axis().lanes();
    assert!((dx - 0.4).abs() < 1e-3);
    assert!((dy - 0.6).abs() < 1e-3);
    assert!((t0 - FRAC_PI_4).abs() < 1e-3);
}

#[test]
fn brush_radial_conic_noop_when_all_transparent() {
    let r = RadialGradient::two_stop_centered(ColorU8::TRANSPARENT, ColorU8::TRANSPARENT);
    let c = ConicGradient::two_stop_centered(ColorU8::TRANSPARENT, ColorU8::TRANSPARENT);
    assert!(Brush::Radial(r).is_noop());
    assert!(Brush::Conic(c).is_noop());
}

#[test]
fn brush_radial_conic_as_solid_is_none() {
    let r = RadialGradient::two_stop_centered(Color::rgb(1.0, 0.0, 0.0), Color::rgb(0.0, 0.0, 1.0));
    let c = ConicGradient::two_stop_centered(Color::rgb(1.0, 0.0, 0.0), Color::rgb(0.0, 0.0, 1.0));
    assert!(Brush::Radial(r).as_solid().is_none());
    assert!(Brush::Conic(c).as_solid().is_none());
}

#[test]
fn brush_variant_tag_distinguishes_hash() {
    let stops = [
        Stop::new(0.0, Color::rgb(1.0, 0.0, 0.0)),
        Stop::new(1.0, Color::rgb(0.0, 0.0, 1.0)),
    ];
    let l = Brush::Linear(LinearGradient::new(0.0, stops));
    let r = Brush::Radial(RadialGradient::new(
        Vec2::splat(0.5),
        Vec2::splat(0.5),
        stops,
    ));
    let c = Brush::Conic(ConicGradient::new(Vec2::splat(0.5), 0.0, stops));
    assert_ne!(h(l.clone()), h(r.clone()));
    assert_ne!(h(r), h(c.clone()));
    assert_ne!(h(l), h(c));
}

#[test]
#[should_panic(expected = "exceeds MAX_STOPS")]
fn radial_too_many_stops_panics() {
    let many: Vec<Stop> = (0..=MAX_STOPS)
        .map(|i| Stop::new(i as f32 / 8.0, Color::WHITE))
        .collect();
    let _ = RadialGradient::new(Vec2::splat(0.5), Vec2::splat(0.5), many);
}

#[test]
fn linear_brush_hash_stable_across_construction() {
    let g0 = LinearGradient::two_stop(0.5, Color::hex(0x336699), Color::hex(0xddaa44));
    let g1 = LinearGradient::two_stop(0.5, Color::hex(0x336699), Color::hex(0xddaa44));
    assert_eq!(h(Brush::Linear(g0)), h(Brush::Linear(g1)));
}
