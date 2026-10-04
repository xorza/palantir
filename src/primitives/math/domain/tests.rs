use crate::internals::panic_probe;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::domain::{self, EPS, is_approx_zero, is_invisible, share_of, vec2};
use crate::primitives::paint::color::RgbaF32;
use glam::Vec2;

/// **The NaN audit.** Every *paint* no-op predicate — "would this
/// put down a texel" — must answer `true` for a NaN anywhere in its
/// inputs, because a NaN that survives the gate goes on to poison a
/// bbox, a damage rect, or a shader lane, and does it silently.
///
/// Deliberately excluded are the predicates that ask a *different*
/// question: `is_approx_zero`, `Size::is_approx_zero`, `Rect::is_approx_zero`,
/// `Corners::is_approx_zero`, and `TranslateScale::is_identity` all mean "is
/// this value ≈ this constant", and they gate **fast paths**, not
/// paint. Answering `true` there would route a NaN *into* the sharp /
/// identity shortcut instead of away from it — the opposite of safe.
/// They are covered by the shape-level gate instead, which drops a
/// NaN before any of them is ever reached.
#[test]
fn every_paint_noop_predicate_treats_nan_as_invisible() {
    use crate::primitives::geometry::mesh::{Mesh, MeshVertex};
    use crate::primitives::geometry::size::Size;
    use crate::primitives::paint::brush::Brush;
    use crate::primitives::paint::color::RgbaF32;
    use crate::primitives::paint::color::rgba_f16::RgbaF16;
    use crate::primitives::paint::shadow::Shadow;
    use crate::primitives::paint::stroke::Stroke;
    use crate::shape::paint::lowered_shadow::LoweredShadow;
    use crate::shape::paint::shape_stroke::ShapeStroke;
    use glam::Vec2;

    const N: f32 = f32::NAN;
    let nan_color = RgbaF32::srgba(0.0, 0.0, 0.0, N);
    // `Mesh::vertex` refuses a non-finite position, so the NaN vertex goes
    // in through the crate's own list: the predicate is what this pins.
    let mut nan_mesh = Mesh::new();
    for pos in [Vec2::new(N, 0.0), Vec2::ZERO, Vec2::X] {
        nan_mesh.vertices.push(MeshVertex::new(pos, RgbaF32::WHITE));
    }
    nan_mesh.triangle(0, 1, 2);

    let cases: &[(&str, bool)] = &[
        ("is_invisible", is_invisible(N)),
        ("Size::is_paint_empty/w", Size::new(N, 4.0).is_paint_empty()),
        ("Size::is_paint_empty/h", Size::new(4.0, N).is_paint_empty()),
        (
            "Rect::is_paint_empty/size",
            Rect::new(0.0, 0.0, N, 4.0).is_paint_empty(),
        ),
        (
            "Rect::is_paint_empty/min",
            Rect::new(N, 0.0, 4.0, 4.0).is_paint_empty(),
        ),
        ("RgbaF32::is_noop", nan_color.is_noop()),
        ("RgbaF16::is_noop", RgbaF16::from(nan_color).is_noop()),
        (
            "Stroke::is_noop/width",
            Stroke::new(RgbaF32::WHITE, N).is_noop(),
        ),
        (
            "Stroke::is_noop/color",
            Stroke::new(nan_color, 2.0).is_noop(),
        ),
        (
            "ShapeStroke::is_noop/width",
            ShapeStroke::from(Stroke::new(RgbaF32::WHITE, N)).is_noop(),
        ),
        (
            "ShapeStroke::is_noop/color",
            ShapeStroke::from(Stroke::new(nan_color, 2.0)).is_noop(),
        ),
        ("Brush::is_noop", Brush::Solid(nan_color).is_noop()),
        (
            "Shadow::is_noop/color",
            Shadow {
                color: nan_color,
                ..Shadow::default()
            }
            .is_noop(),
        ),
        (
            "Shadow::is_noop/blur",
            Shadow {
                color: RgbaF32::WHITE,
                blur: N,
                ..Shadow::default()
            }
            .is_noop(),
        ),
        (
            "Shadow::is_noop/offset",
            Shadow {
                color: RgbaF32::WHITE,
                offset: Vec2::new(N, 0.0),
                ..Shadow::default()
            }
            .is_noop(),
        ),
        (
            "Shadow::is_noop/spread",
            Shadow {
                color: RgbaF32::WHITE,
                spread: N,
                ..Shadow::default()
            }
            .is_noop(),
        ),
        ("Mesh::is_noop", nan_mesh.is_noop()),
        // Chrome has no record-level gate to fall back on — it does
        // not pass through `Shapes::add` — so these four are the
        // only thing standing between a NaN `Background` and the
        // shader.
        (
            "RgbaF16::is_noop/red",
            RgbaF16::from(RgbaF32::srgba(N, 0.0, 0.0, 1.0)).is_noop(),
        ),
        (
            "RgbaF32::is_noop/red",
            RgbaF32::srgba(N, 0.0, 0.0, 1.0).is_noop(),
        ),
        (
            "LoweredShadow::is_noop/blur",
            LoweredShadow::from(Shadow {
                color: RgbaF32::WHITE,
                blur: N,
                ..Shadow::default()
            })
            .is_noop(),
        ),
        (
            "LoweredShadow::is_noop/offset",
            LoweredShadow::from(Shadow {
                color: RgbaF32::WHITE,
                offset: Vec2::new(N, 0.0),
                ..Shadow::default()
            })
            .is_noop(),
        ),
    ];

    let missed: Vec<&str> = cases
        .iter()
        .filter(|(_, is_noop)| !is_noop)
        .map(|(label, _)| *label)
        .collect();
    assert!(
        missed.is_empty(),
        "these paint no-op predicates let a NaN through: {missed:?}",
    );
}

#[test]
fn approx_zero_handles_boundary_sign_and_nan() {
    let cases: &[(&str, f32, bool)] = &[
        ("exact_zero", 0.0, true),
        ("neg_zero", -0.0, true),
        ("at_eps", EPS, true),
        ("at_neg_eps", -EPS, true),
        ("just_above_eps", EPS * 1.1, false),
        ("just_below_neg_eps", -EPS * 1.1, false),
        ("nan", f32::NAN, false),
    ];
    for (label, v, want) in cases {
        assert_eq!(is_approx_zero(*v), *want, "case: {label}");
    }
}

/// A share of a collapsed divisor is nothing, not a huge number: zero,
/// sub-`EPS` and negative divisors all answer 0. Above the floor it is the
/// plain quotient.
#[test]
fn share_of_answers_zero_for_a_collapsed_divisor() {
    for (label, n, d, want) in [
        ("plain", 3.0, 4.0, 0.75),
        ("whole", 5.0, 5.0, 1.0),
        ("zero divisor", 3.0, 0.0, 0.0),
        ("sub-eps divisor", 3.0, EPS * 0.5, 0.0),
        ("negative divisor", 3.0, -4.0, 0.0),
    ] {
        assert_eq!(share_of(n, d), want, "{label}");
    }
}

/// Two points coincide within `EPS` of Euclidean distance, inclusive.
/// Measured from the origin so the offsets are exact: `EPS` on one axis
/// is on the boundary, and `EPS` on both is √2 · `EPS` away.
#[test]
fn vec2_approx_eq_is_euclidean_and_inclusive() {
    for (label, b, want) in [
        ("same", Vec2::ZERO, true),
        ("half eps on x", Vec2::new(EPS * 0.5, 0.0), true),
        ("one eps on y", Vec2::new(0.0, EPS), true),
        ("two eps on x", Vec2::new(EPS * 2.0, 0.0), false),
        ("eps on both axes", Vec2::splat(EPS), false),
        ("half eps on both axes", Vec2::splat(EPS * 0.5), true),
    ] {
        assert_eq!(vec2::approx_eq(Vec2::ZERO, b), want, "{label}");
        assert_eq!(vec2::approx_eq(b, Vec2::ZERO), want, "{label}: swapped");
    }
}

/// A 20 px band on a 120 px track leaves 100 px of travel, offset by
/// 10 px at each end: 10 → 0.0, 60 → 0.5, 110 → 1.0. Outside the track
/// the share runs past 0..1, which is the caller's to pin. A band at
/// least as wide as its track leaves no travel, so there is no share.
#[test]
fn band_fraction_offsets_by_half_the_band() {
    let cases: &[(f32, f32)] = &[
        (10.0, 0.0),
        (35.0, 0.25),
        (60.0, 0.5),
        (110.0, 1.0),
        (0.0, -0.1),
        (120.0, 1.1),
    ];
    for &(pos, want) in cases {
        let got = domain::band_fraction(pos, 120.0, 20.0);
        assert_eq!(got, want, "band_fraction({pos}) = {got}, want {want}");
    }
    assert_eq!(domain::band_fraction(15.0, 20.0, 20.0), 0.0);
    assert_eq!(domain::band_fraction(15.0, 10.0, 20.0), 0.0);
    // Per component on a point: each axis is the scalar answer over its
    // own extent and band, so a 60 on the 120 track beside a 20 on a 40
    // track with a 10 band (30 px of travel from 5) reads (0.5, 0.5).
    let point = vec2::band_fraction(
        Vec2::new(60.0, 20.0),
        Vec2::new(120.0, 40.0),
        Vec2::new(20.0, 10.0),
    );
    assert_eq!(point, Vec2::splat(0.5), "{point}");
}

/// Every validating kind at its ends: each value it takes comes back
/// bit for bit, from the checker as from a `const` context, and each value
/// it refuses panics with the kind's rule.
#[test]
fn each_validating_kind_takes_its_domain_and_refuses_the_rest() {
    type Kind = (&'static str, fn(f32) -> bool, fn(f32) -> f32);
    const IN_CONST: f32 = domain::length(2.0);
    assert_eq!(IN_CONST, 2.0);

    let rows: [(Kind, &[f32], &[f32]); 6] = [
        (
            (domain::OFFSET_RULE, domain::is_offset, domain::offset),
            &[-1e30, -1.0, 0.0, 1e30],
            &[f32::NAN, f32::INFINITY, f32::NEG_INFINITY],
        ),
        (
            (domain::LENGTH_RULE, domain::is_length, domain::length),
            &[0.0, -0.0, 1.0, 1e30],
            &[-EPS, -1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY],
        ),
        (
            (domain::EXTENT_RULE, domain::is_extent, domain::extent),
            &[0.0, 1.0, f32::INFINITY],
            &[-1.0, f32::NAN, f32::NEG_INFINITY],
        ),
        (
            (domain::GAP_RULE, domain::is_gap, domain::gap),
            &[0.0, 1.0, 65504.0],
            &[65505.0, -1.0, f32::NAN, f32::INFINITY],
        ),
        (
            (domain::POSITIVE_RULE, domain::is_positive, domain::positive),
            &[f32::MIN_POSITIVE, 1.0, 1e30],
            &[0.0, -0.0, -1.0, f32::NAN, f32::INFINITY],
        ),
        (
            (domain::ANGLE_RULE, domain::is_angle, domain::angle),
            &[-7.0, 0.0, 7.0],
            &[f32::NAN, f32::INFINITY, f32::NEG_INFINITY],
        ),
    ];
    for ((rule, is_kind, kind), takes, refuses) in rows {
        for &v in takes {
            assert!(is_kind(v), "{rule}: takes {v}");
            assert_eq!(kind(v).to_bits(), v.to_bits(), "{rule}: returns {v}");
        }
        for &v in refuses {
            assert!(!is_kind(v), "{rule}: refuses {v}");
            panic_probe::assert_panics_with(rule, || kind(v));
        }
    }
}

/// The kinds over other types than one `f32`: a colour checks every
/// channel and lets HDR values through, a count starts at one, a
/// power of two stays inside its maximum, and the two-axis twins refuse
/// a value one bad axis spoils.
#[test]
fn compound_kinds_check_every_part() {
    let hdr = RgbaF32::new(2.0, -1.0, 0.0, 1.0);
    assert_eq!(domain::color(hdr), hdr);
    for channel in 0..4 {
        for bad in [f32::NAN, f32::INFINITY] {
            let mut lanes = [0.5; 4];
            lanes[channel] = bad;
            let c = RgbaF32::new(lanes[0], lanes[1], lanes[2], lanes[3]);
            assert!(!domain::is_color(c), "channel {channel} = {bad}");
            panic_probe::assert_panics_with("a color must have finite channels", || {
                domain::color(c)
            });
        }
    }

    assert_eq!(domain::count(1), 1);
    assert_eq!(domain::count(u32::MAX), u32::MAX);
    panic_probe::assert_panics_with("a count must be at least 1", || domain::count(0));

    for n in [1, 2, 4, 8, 16] {
        assert_eq!(domain::power_of_two_in(n, 16), n);
    }
    for n in [0, 3, 12, 32] {
        assert!(!domain::is_power_of_two_in(n, 16), "{n}");
        panic_probe::assert_panics_with("must be a power of two", || {
            domain::power_of_two_in(n, 16)
        });
    }

    assert_eq!(vec2::offset(Vec2::new(1.0, -1.0)), Vec2::new(1.0, -1.0));
    assert!(!vec2::is_length(Vec2::new(1.0, -1.0)));
    panic_probe::assert_panics_with(domain::LENGTH_RULE, || {
        vec2::length(Vec2::new(0.0, f32::NAN))
    });
    panic_probe::assert_panics_with(domain::OFFSET_RULE, || {
        vec2::offset(Vec2::new(f32::INFINITY, 0.0))
    });
}

/// A range's ends are validated and its order coerced: a reversed range
/// comes back ascending, and an infinite or NaN end panics.
#[test]
fn range_orders_finite_ends_and_refuses_the_rest() {
    assert_eq!(domain::range(3.0..=1.0), 1.0..=3.0);
    assert_eq!(domain::range(-1.0..=2.0), -1.0..=2.0);
    assert_eq!(domain::range(5.0..=5.0), 5.0..=5.0);
    for bad in [f64::NAN..=1.0, 0.0..=f64::INFINITY, f64::NEG_INFINITY..=0.0] {
        assert!(!domain::is_range(&bad), "{bad:?}");
        panic_probe::assert_panics_with("a range must have finite ends", || {
            domain::range(bad.clone())
        });
    }
}

/// The coercing kinds are total. A fraction clamps to the end it overshot
/// and reads every non-finite value as `0`; a turn wraps by whole turns
/// (`1.25 - 1 = 0.25`, `-0.75 + 1 = 0.25`) and a value a hair below zero,
/// whose wrap rounds up to `1.0`, lands on `0`; an index clamps to the last
/// item and is `None` with no items.
#[test]
fn coercing_kinds_are_total() {
    for (v, want) in [
        (0.0, 0.0),
        (0.25, 0.25),
        (1.0, 1.0),
        (-0.3, 0.0),
        (1.7, 1.0),
        (f32::NAN, 0.0),
        (f32::INFINITY, 0.0),
        (f32::NEG_INFINITY, 0.0),
    ] {
        assert_eq!(domain::fraction(v), want, "fraction({v})");
    }
    for (v, want) in [(0.0, true), (-0.0, true), (1.0, true), (1.0001, false)] {
        assert_eq!(domain::is_fraction(v), want, "is_fraction({v})");
    }
    assert!(!domain::is_fraction(f32::NAN));

    for (v, want) in [
        (0.0, 0.0),
        (0.25, 0.25),
        (1.25, 0.25),
        (-0.75, 0.25),
        (1.0, 0.0),
        (-1.0, 0.0),
        (-1e-10, 0.0),
        (f32::NAN, 0.0),
        (f32::INFINITY, 0.0),
        (f32::NEG_INFINITY, 0.0),
    ] {
        assert_eq!(domain::turn(v), want, "turn({v})");
    }

    for (i, len, want) in [
        (0, 3, Some(0)),
        (2, 3, Some(2)),
        (3, 3, Some(2)),
        (usize::MAX, 3, Some(2)),
        (0, 0, None),
        (5, 0, None),
    ] {
        assert_eq!(domain::index(i, len), want, "index({i}, {len})");
    }
}

/// The screen every caller-supplied share passes: in-range values are
/// untouched, out-of-range ones clamp to the end they overshot, and a
/// value that names no share at all takes the caller's neutral rather
/// than an end.
///
/// The infinities matter as much as NaN — `f32::clamp` maps them to an
/// end, which states a share the caller never meant.
#[test]
fn fraction_or_clamps_in_range_and_falls_back_outside_the_finite() {
    let cases: &[(f32, f32, f32)] = &[
        (0.0, 0.5, 0.0),
        (0.25, 0.5, 0.25),
        (1.0, 0.5, 1.0),
        (-0.3, 0.5, 0.0),
        (1.7, 0.5, 1.0),
        (f32::NAN, 0.5, 0.5),
        (f32::INFINITY, 0.5, 0.5),
        (f32::NEG_INFINITY, 0.5, 0.5),
        // The neutral is the caller's: the same non-finite input reads
        // as empty for a progress bar and as centred for a splitter.
        (f32::NAN, 0.0, 0.0),
        (f32::INFINITY, 1.0, 1.0),
    ];
    assert_eq!(
        vec2::fraction_or(Vec2::new(1.7, f32::NAN), Vec2::new(0.5, 0.25)),
        Vec2::new(1.0, 0.25),
        "each component clamps or falls back on its own",
    );
    for &(value, fallback, want) in cases {
        assert_eq!(
            domain::fraction_or(value, fallback),
            want,
            "fraction_or({value}, {fallback})",
        );
    }
}

/// A themed length floors at the widget's minimum and keeps anything
/// above it; scalar equality is the zero test on the difference, so two
/// halves of `EPS` either side of zero are exactly `EPS` apart.
#[test]
fn length_at_least_floors_and_approx_eq_is_inclusive() {
    assert_eq!(domain::length_at_least(0.5, 1.0), 1.0);
    assert_eq!(domain::length_at_least(3.0, 1.0), 3.0);
    assert_eq!(domain::length_at_least(0.0, 0.0), 0.0);
    assert_eq!(
        vec2::length_at_least(Vec2::new(0.5, 3.0), Vec2::ONE),
        Vec2::new(1.0, 3.0),
    );

    assert!(domain::approx_eq(0.0, EPS));
    assert!(domain::approx_eq(-EPS * 0.5, EPS * 0.5));
    assert!(!domain::approx_eq(0.0, EPS * 2.0));
    assert!(!domain::approx_eq(f32::NAN, f32::NAN));
}
