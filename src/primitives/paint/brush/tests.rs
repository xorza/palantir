use crate::animation::animatable::Animatable;
use crate::internals::panic_probe;
use crate::primitives::math::domain;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::brush::gradient::conic_geometry::ConicGradient;
use crate::primitives::paint::brush::gradient::linear_geometry::LinearGradient;
use crate::primitives::paint::brush::gradient::radial_geometry::RadialGradient;
use crate::primitives::paint::brush::gradient::stops::{GradientStops, MAX_STOPS, Stop};
use crate::primitives::paint::brush::gradient::{Gradient, Interpolation, Spread};
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use glam::Vec2;
use ron::ser;
use std::collections::hash_map::DefaultHasher;
use std::f32::consts::{FRAC_PI_4, PI};
use std::fmt;
use std::fmt::Write as _;
use std::hash::{Hash, Hasher};

/// `LinearGradient::Hash` is what `shapes::lower` folds into a record's gradient content hash.
fn h(g: &LinearGradient) -> u64 {
    let mut s = DefaultHasher::new();
    g.hash(&mut s);
    s.finish()
}

#[derive(Debug, ::serde::Deserialize)]
struct StopsDocument {
    stops: GradientStops,
}

/// `LinearGradient::Hash` feeds shape identity, so `±0.0` and NaN variants must collapse.
#[test]
fn linear_gradient_hash_tracks_canonical_content() {
    let nan_a = f32::from_bits(0x7fc0_0001);
    let nan_b = f32::from_bits(0x7fc0_0002);
    assert!(nan_a.is_nan() && nan_b.is_nan());
    let cases: &[(&str, LinearGradient, LinearGradient)] = &[
        (
            "angle_neg_zero_eq_pos_zero",
            LinearGradient::two_stop(0.0, RgbaF32::BLACK, RgbaF32::WHITE),
            LinearGradient::two_stop(-0.0, RgbaF32::BLACK, RgbaF32::WHITE),
        ),
        (
            "angle_nan_bit_patterns_collapse",
            LinearGradient::two_stop(nan_a, RgbaF32::BLACK, RgbaF32::WHITE),
            LinearGradient::two_stop(nan_b, RgbaF32::BLACK, RgbaF32::WHITE),
        ),
        (
            "stop_offset_neg_zero_eq_pos_zero",
            LinearGradient::new(
                0.0,
                [
                    Stop::new(0.0, RgbaF32::BLACK),
                    Stop::new(1.0, RgbaF32::WHITE),
                ],
            ),
            LinearGradient::new(
                0.0,
                [
                    Stop::new(-0.0, RgbaF32::BLACK),
                    Stop::new(1.0, RgbaF32::WHITE),
                ],
            ),
        ),
    ];
    for (label, x, y) in cases {
        assert_eq!(h(x), h(y), "case: {label}");
    }

    let two_stops = LinearGradient::two_stop(0.0, RgbaF32::BLACK, RgbaF32::WHITE);
    let three_stops = LinearGradient::builder(0.0)
        .stop(0.0, RgbaF32::BLACK)
        .stop(0.5, RgbaF32::new(0.5, 0.5, 0.5, 1.0))
        .stop(1.0, RgbaF32::WHITE)
        .build();
    let recolored = LinearGradient::two_stop(0.0, RgbaF32::BLACK, RgbaF32::new(1.0, 0.0, 0.0, 1.0));
    assert_ne!(h(&two_stops), h(&three_stops));
    assert_ne!(h(&two_stops), h(&recolored));
}

#[test]
fn authoring_values_convert_to_their_brush_variants() {
    let color = RgbaF32::WHITE;
    let linear = LinearGradient::two_stop(0.25, RgbaF32::BLACK, RgbaF32::WHITE);
    let radial = RadialGradient::two_stop(RgbaF32::BLACK, RgbaF32::WHITE);
    let conic = ConicGradient::two_stop(RgbaF32::BLACK, RgbaF32::WHITE);
    let linear_builder = LinearGradient::builder(0.25)
        .stop(0.0, RgbaF32::BLACK)
        .stop(1.0, RgbaF32::WHITE);

    assert_eq!(Brush::from(color), Brush::Solid(color));
    // sRGB 0 and 255 decode to exactly 0.0 and 1.0; alpha is straight: 51 / 255 = 0.2.
    let bytes = SrgbaU8::new(255, 0, 255, 51);
    let decoded = RgbaF32::new(1.0, 0.0, 1.0, 0.2);
    assert_eq!(Brush::from(bytes), Brush::Solid(decoded));
    assert_eq!(Brush::from(linear.clone()), Brush::Linear(linear));
    assert_eq!(Brush::from(radial.clone()), Brush::Radial(radial));
    assert_eq!(Brush::from(conic.clone()), Brush::Conic(conic));
    assert_eq!(
        Brush::from(linear_builder.clone()),
        Brush::Linear(linear_builder.build()),
    );
}

#[test]
fn solid_solid_animatable_lerp_matches_color() {
    let a = RgbaF32::BLACK;
    let b = RgbaF32::WHITE;
    let mid_color = RgbaF32::lerp(a, b, 0.5);
    let mid_brush = Brush::lerp(Brush::Solid(a), Brush::Solid(b), 0.5);
    assert_eq!(mid_brush, Brush::Solid(mid_color));
}

#[test]
fn solid_is_noop_iff_color_is_noop() {
    assert!(Brush::Solid(RgbaF32::TRANSPARENT).is_noop());
    assert!(!Brush::Solid(RgbaF32::BLACK).is_noop());
}

/// Two through `MAX_STOPS` stops hold at every door: `GradientStops::new`, a gradient's builder (rejects a ninth, too few at `build`), its `new`, and deserialization.
#[test]
fn gradient_stop_count_is_enforced_by_construction_and_deserialization() {
    let offset = |index: usize, count: usize| index as f32 / count.max(1) as f32;
    let stops = |count: usize| {
        (0..count)
            .map(|index| Stop::new(offset(index, count), RgbaF32::WHITE))
            .collect::<Vec<_>>()
    };
    let serialized = |count: usize| {
        let mut document = String::from("(stops: [");
        for index in 0..count {
            let offset = offset(index, count);
            write!(document, "(offset: {offset}, color: \"#ffffff\"),").unwrap();
        }
        document.push_str("])");
        document
    };
    let built = |count: usize| {
        let mut builder = LinearGradient::builder(0.0);
        for index in 0..count {
            builder = builder.stop(offset(index, count), RgbaF32::WHITE);
        }
        builder.build()
    };
    let radial =
        |count: usize| RadialGradient::new(Vec2::splat(0.5), Vec2::splat(0.5), stops(count));

    for (count, rejection) in [
        (0, Some("gradient requires at least 2 stops, got 0")),
        (1, Some("gradient requires at least 2 stops, got 1")),
        (2, None),
        (8, None),
        (9, Some("gradient stop count exceeds MAX_STOPS = 8")),
    ] {
        let deserialized =
            ron::from_str::<StopsDocument>(&serialized(count)).map(|value| value.stops.len());
        assert_eq!(
            deserialized.is_ok(),
            rejection.is_none(),
            "deserializer count {count}",
        );
        if let Some(message) = rejection {
            panic_probe::assert_panics_with(message, || GradientStops::new(stops(count)));
            panic_probe::assert_panics_with(message, || built(count));
            panic_probe::assert_panics_with(message, || radial(count));
        } else {
            assert!((2..=MAX_STOPS).contains(&count));
            assert_eq!(GradientStops::new(stops(count)).len(), count);
            assert_eq!(built(count).ramp.stops.len(), count);
            assert_eq!(radial(count).ramp.stops.len(), count);
            assert_eq!(deserialized.unwrap(), count);
        }
    }
}

/// Each kind's `two_stop` runs 0 → 1 with `Spread::Pad`; Oklab for linear and radial, linear for conic. It paints unless both stops are transparent, is never solid, and setters change only what they name.
#[test]
fn two_stop_gradients_take_their_kind_defaults() {
    fn check<G: Clone + fmt::Debug + PartialEq>(
        kind: &str,
        two_stop: impl Fn(RgbaF32, RgbaF32) -> Gradient<G>,
        interpolation: Interpolation,
    ) where
        Brush: From<Gradient<G>>,
    {
        let g = two_stop(RgbaF32::BLACK, RgbaF32::WHITE);
        let offsets = [g.ramp.stops[0].offset(), g.ramp.stops[1].offset()];
        assert_eq!((g.ramp.stops.len(), offsets), (2, [0.0, 1.0]), "{kind}");
        assert_eq!(
            (g.spread, g.ramp.interpolation),
            (Spread::Pad, interpolation),
            "{kind}"
        );
        let brush = Brush::from(g.clone());
        assert!(!brush.is_noop(), "{kind}");
        assert_eq!(brush.as_solid(), None, "{kind}");

        let clear = two_stop(RgbaF32::TRANSPARENT, RgbaF32::WHITE.with_alpha(0.0));
        assert!(Brush::from(clear).is_noop(), "{kind}: all transparent");

        let other = match interpolation {
            Interpolation::Linear => Interpolation::Oklab,
            Interpolation::Oklab => Interpolation::Linear,
        };
        let overridden = g
            .clone()
            .with_spread(Spread::Repeat)
            .with_interpolation(other);
        assert_eq!(
            (overridden.spread, overridden.ramp.interpolation),
            (Spread::Repeat, other),
            "{kind}"
        );
        assert_eq!(overridden.ramp.stops, g.ramp.stops, "{kind}");
        assert_eq!(overridden.geometry, g.geometry, "{kind}");
    }
    check(
        "linear",
        |a, b| LinearGradient::two_stop(0.0, a, b),
        Interpolation::Oklab,
    );
    check("radial", RadialGradient::two_stop, Interpolation::Oklab);
    check("conic", ConicGradient::two_stop, Interpolation::Linear);

    // A radial gradient defaults to the centred circle: centre and radius 0.5, exact in f16.
    let radial = RadialGradient::two_stop(RgbaF32::WHITE, RgbaF32::BLACK);
    assert_eq!(radial.geometry.center, Vec2::splat(0.5));
    assert_eq!(radial.geometry.radius, Vec2::splat(0.5));
    assert_eq!(radial.axis().lanes(), [0.5, 0.5, 0.5, 0.5]);
}

/// In code a stop offset is coerced (out of range clamps, non-finite reads 0) and a non-finite colour channel panics. A file refuses any offset outside `0..=1`.
#[test]
fn stop_offsets_coerce_in_code_and_are_refused_in_files() {
    for (offset, want) in [
        (f32::NAN, 0.0),
        (f32::INFINITY, 0.0),
        (f32::NEG_INFINITY, 0.0),
        (-0.5, 0.0),
        (1.5, 1.0),
        (0.5, 128.0 / 255.0),
    ] {
        assert_eq!(Stop::new(offset, RgbaF32::WHITE).offset(), want, "{offset}");
    }
    panic_probe::assert_panics_with("a color must have finite channels", || {
        Stop::new(0.5, RgbaF32::new(f32::NAN, 0.0, 0.0, 1.0))
    });

    for literal in ["NaN", "inf", "-inf", "-0.5", "1.5"] {
        let document = format!(
            "(stops: [\
               (offset: {literal}, color: \"#ffffff\"),\
               (offset: 1.0, color: \"#000000\"),\
             ])"
        );
        let error = ron::from_str::<StopsDocument>(&document).unwrap_err();
        assert!(
            error.to_string().contains(domain::FRACTION_RULE),
            "{literal} produced unexpected error: {error}",
        );
    }
}

/// Every kind round-trips; a file whose geometry breaks its kind is a deserialization error: angles are angles, centres offsets, radial radius a length per axis.
#[test]
fn every_gradient_variant_round_trips_and_files_refuse_bad_geometry() {
    #[derive(Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
    struct BrushDocument {
        brush: Brush,
    }

    let brushes = [
        Brush::Linear(LinearGradient::two_stop(
            0.25,
            RgbaF32::BLACK,
            RgbaF32::WHITE,
        )),
        Brush::Radial(RadialGradient::two_stop(RgbaF32::BLACK, RgbaF32::WHITE)),
        Brush::Conic(ConicGradient::two_stop(RgbaF32::BLACK, RgbaF32::WHITE)),
    ];
    for brush in brushes {
        let document = BrushDocument { brush };
        let encoded = ser::to_string(&document).expect("serialize valid gradient");
        let decoded = ron::from_str::<BrushDocument>(&encoded).expect("deserialize valid gradient");
        assert_eq!(decoded, document);
    }

    let stops = r##""stops":[(offset:0.0,color:"#000000"),(offset:1.0,color:"#ffffff")],"interpolation":Oklab,"spread":Pad"##;
    for (geometry, rule) in [
        (r#"Linear({"angle":inf,"#, domain::ANGLE_RULE),
        (
            r#"Radial({"center":(NaN,0.5),"radius":(0.5,0.5),"#,
            domain::OFFSET_RULE,
        ),
        (
            r#"Radial({"center":(0.5,0.5),"radius":(-0.5,0.5),"#,
            domain::LENGTH_RULE,
        ),
        (
            r#"Conic({"center":(0.5,inf),"start_angle":0.0,"#,
            domain::OFFSET_RULE,
        ),
        (
            r#"Conic({"center":(0.5,0.5),"start_angle":NaN,"#,
            domain::ANGLE_RULE,
        ),
    ] {
        let document = format!("(brush:{geometry}{stops}}}))");
        let error = ron::from_str::<BrushDocument>(&document).unwrap_err();
        assert!(error.to_string().contains(rule), "{geometry}: {error}");
    }
}

#[test]
fn gradient_builders_preserve_geometry_stops_and_options() {
    let linear = LinearGradient::builder(PI / 2.0)
        .stop(-1.0, RgbaF32::hex(0x000000))
        .stop(0.5, RgbaF32::hex(0x808080))
        .stop(2.0, RgbaF32::hex(0xffffff))
        .spread(Spread::Reflect)
        .interpolation(Interpolation::Linear)
        .build();
    assert_eq!(linear.geometry.angle, PI / 2.0);
    assert_eq!(linear.ramp.stops.len(), 3);
    assert_eq!(linear.ramp.stops[0].offset(), 0.0);
    assert_eq!(linear.ramp.stops[1].offset(), 128.0 / 255.0);
    assert_eq!(linear.ramp.stops[2].offset(), 1.0);
    assert_eq!(linear.spread, Spread::Reflect);
    assert_eq!(linear.ramp.interpolation, Interpolation::Linear);

    let center = Vec2::new(0.25, 0.75);
    let radius = Vec2::new(0.4, 0.6);
    let radial = RadialGradient::builder(center, radius)
        .stop(0.0, RgbaF32::BLACK)
        .stop(1.0, RgbaF32::WHITE)
        .build();
    assert_eq!(radial.geometry.center, center);
    assert_eq!(radial.geometry.radius, radius);
    assert_eq!(radial.ramp.interpolation, Interpolation::Oklab);

    let conic = ConicGradient::builder(center, FRAC_PI_4)
        .stop(0.0, RgbaF32::BLACK)
        .stop(1.0, RgbaF32::WHITE)
        .build();
    assert_eq!(conic.geometry.center, center);
    assert_eq!(conic.geometry.start_angle, FRAC_PI_4);
    assert_eq!(conic.ramp.interpolation, Interpolation::Linear);
}

#[test]
fn linear_brush_animatable_snaps_on_t_one() {
    let g0 = LinearGradient::two_stop(0.0, RgbaF32::BLACK, RgbaF32::WHITE);
    let g1 = LinearGradient::two_stop(0.0, RgbaF32::WHITE, RgbaF32::BLACK);
    let a = Brush::Linear(g0);
    let b = Brush::Linear(g1);
    assert_eq!(Brush::lerp(a.clone(), b.clone(), 0.5), a);
    assert_eq!(Brush::lerp(a, b.clone(), 1.0), b);
}

fn assert_spring_normalizes_to_target(mut current: Brush, target: &Brush) {
    let mut velocity = Brush::Solid(RgbaF32::srgba(0.25, -0.5, 0.75, 1.0));
    current.normalize_for_spring(target, &mut velocity);
    assert_eq!(current, *target);
    assert_eq!(velocity, Brush::TRANSPARENT);
}

#[test]
fn gradient_brush_spring_normalization_is_direction_independent() {
    let solid = Brush::Solid(RgbaF32::hex(0x336699));
    let gradients = [
        Brush::Linear(LinearGradient::two_stop(
            0.25,
            RgbaF32::BLACK,
            RgbaF32::WHITE,
        )),
        Brush::Radial(RadialGradient::two_stop(RgbaF32::BLACK, RgbaF32::WHITE)),
        Brush::Conic(ConicGradient::two_stop(RgbaF32::BLACK, RgbaF32::WHITE)),
    ];
    let replacement_gradients = [
        Brush::Linear(LinearGradient::two_stop(
            0.75,
            RgbaF32::WHITE,
            RgbaF32::BLACK,
        )),
        Brush::Radial(RadialGradient::two_stop(RgbaF32::WHITE, RgbaF32::BLACK)),
        Brush::Conic(ConicGradient::two_stop(RgbaF32::WHITE, RgbaF32::BLACK)),
    ];

    for gradient in &gradients {
        assert_spring_normalizes_to_target(solid.clone(), &gradient.clone());
        assert_spring_normalizes_to_target(gradient.clone(), &solid.clone());
    }
    for source in &gradients {
        for target in &replacement_gradients {
            assert_spring_normalizes_to_target(source.clone(), &target.clone());
        }
    }
}

#[test]
fn conic_axis_packs_start_angle() {
    let g = ConicGradient::new(
        Vec2::new(0.4, 0.6),
        FRAC_PI_4,
        [
            Stop::new(0.0, RgbaF32::srgb(1.0, 0.0, 0.0)),
            Stop::new(1.0, RgbaF32::srgb(0.0, 0.0, 1.0)),
        ],
    );
    // The axis packs to f16 (10 mantissa bits): 0.4 is 1638.4 steps of 2^-12 → 1638, 0.6 is 1228.8 of 2^-11 → 1229, π/4 is 1608.5 of 2^-11 → 1608.
    assert_eq!(
        g.axis().lanes(),
        [1638.0 / 4096.0, 1229.0 / 2048.0, 1608.0 / 2048.0, 0.0],
    );
}
