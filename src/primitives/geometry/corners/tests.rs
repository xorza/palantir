use crate::internals::panic_probe;
use crate::primitives::geometry::corners::*;
use crate::primitives::math::domain::EPS;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::packed::serde::internals::{from_ron, ron_text};

#[test]
fn to_array_reads_clockwise_from_the_top_left() {
    assert_eq!(
        Corners::new(1.0, 2.0, 3.0, 4.0).to_array(),
        [1.0, 2.0, 3.0, 4.0]
    );
}

/// `f = min(side / sum of its two radii)`, hand-computed per row; every radius scales by `f` when below 1.
#[test]
fn fit_to_scales_overlapping_radii_by_the_tightest_side() {
    let size = Size::new(100.0, 40.0);
    let rows = [
        // f = 40 / (9999 + 9999) on the short sides.
        ("all 9999", Corners::all(9999.0), [20.0; 4]),
        // f = 40 / 60.
        ("all 30", Corners::all(30.0), [20.0; 4]),
        // Sides: top 40 of 100, bottom 0, left 30 of 40, right 10: f ≥ 1.
        (
            "tl 30, tr 10",
            Corners::new(30.0, 10.0, 0.0, 0.0),
            [30.0, 10.0, 0.0, 0.0],
        ),
        // Left side: 40 / (40 + 40) = 0.5.
        (
            "tl 40, bl 40",
            Corners::new(40.0, 0.0, 0.0, 40.0),
            [20.0, 0.0, 0.0, 20.0],
        ),
        ("sharp", Corners::ZERO, [0.0; 4]),
    ];
    for (label, corners, want) in rows {
        assert_eq!(corners.fit_to(size, 1.0).to_array(), want, "{label}");
    }
    // Scaled first, in f32: 9999 × 8 exceeds f16's 65504 yet a 100×40 box at 8× (800×320) fits it to 160.
    assert_eq!(
        Corners::all(9999.0)
            .fit_to(Size::new(800.0, 320.0), 8.0)
            .to_array(),
        [160.0; 4],
    );
}

/// The inner radii fit first, then lose the larger adjacent inset. On 100×40 the radii 30, 10, 0,
/// 40 fit by `40 / 70` to 17.1, 5.7, 0, 22.9 (in f16); padding left 4, top 6, right 2, bottom 8
/// takes 6, 6, 8 and 8 from them, floored at zero.
#[test]
fn inner_fits_then_takes_the_larger_adjacent_inset() {
    let size = Size::new(100.0, 40.0);
    let corners = Corners::new(30.0, 10.0, 0.0, 40.0);
    let [tl, tr, _, bl] = corners.fit_to(size, 1.0).to_array();
    let inner = corners.inner(size, Spacing::new(4.0, 6.0, 2.0, 8.0));
    let want = Corners::new(tl - 6.0, (tr - 6.0).max(0.0), 0.0, bl - 8.0);
    assert_eq!(inner, want);
    assert!(tr < 6.0, "premise: one corner floors at zero");
    // A border is the uniform inset.
    assert_eq!(
        Corners::all(8.0).inner(size, Spacing::all(2.0)),
        Corners::all(6.0)
    );
}

/// Each product is the corner times the factor; one past the lanes saturates at 65504, as
/// 40000 × 2 does, and a factor that is no length panics.
#[test]
fn scaled_by_multiplies_each_corner() {
    let c = Corners::new(2.0, 4.0, 6.0, 8.0).scaled_by(1.5);
    assert_eq!(c.to_array(), [3.0, 6.0, 9.0, 12.0]);
    assert_eq!(
        Corners::new(40000.0, 1.0, 0.0, 2.0)
            .scaled_by(2.0)
            .to_array(),
        [65504.0, 2.0, 0.0, 4.0]
    );
    for bad in [-1.0, f32::NAN, f32::INFINITY] {
        panic_probe::assert_panics_with("a corner scale must be finite", || {
            Corners::all(1.0).scaled_by(bad)
        });
    }
}

/// Every constructor refuses a radius that is no length or packs past f16, on any lane: NaN, an
/// infinity, -1 and 65520, the first magnitude f16 rounds to infinity. 65519 rounds down to
/// 65504 and is taken. A derived radius saturates where an authored one is refused, and still
/// refuses NaN and a negative radius.
#[test]
fn a_radius_past_its_lane_is_refused_where_it_is_built() {
    let rule = <Corners as LaneCodec>::LANE_RULE;
    for bad in [f32::NAN, f32::INFINITY, -1.0, 65520.0] {
        for lane in 0..4 {
            let mut lanes = [1.0, 2.0, 3.0, 4.0];
            lanes[lane] = bad;
            panic_probe::assert_panics_with(rule, || {
                Corners::new(lanes[0], lanes[1], lanes[2], lanes[3])
            });
        }
        let builds: [fn(f32) -> Corners; 5] = [
            Corners::all,
            Corners::top,
            Corners::bottom,
            Corners::left,
            Corners::right,
        ];
        for build in builds {
            panic_probe::assert_panics_with(rule, || build(bad));
        }
    }
    assert_eq!(Corners::all(65519.0).to_array(), [65504.0; 4]);
    assert_eq!(
        Corners::saturating([70000.0, f32::INFINITY, 0.0, 1.0]).to_array(),
        [65504.0, 65504.0, 0.0, 1.0]
    );
    for bad in [f32::NAN, -1.0] {
        panic_probe::assert_panics_with(rule, || Corners::saturating([0.0, bad, 0.0, 0.0]));
    }
}

/// A NaN edge is reported per lane: the four share one `u64`, so checking only the first would pass a NaN bottom corner.
#[test]
fn a_nan_on_any_corner_is_screened() {
    assert!(!Corners::all(4.0).has_nan(), "a whole corner set is finite");
    for lane in 0..4 {
        let mut lanes = [1.0, 2.0, 3.0, 4.0];
        lanes[lane] = f32::NAN;
        assert!(
            NanCheck::has_nan(&Corners(F16x4::from_lanes(lanes))),
            "lane {lane} went unseen",
        );
    }
}

/// The bit-trick path in `is_approx_zero`: ±0, sub-EPS, at-EPS, above-EPS and NaN lanes classify correctly.
#[test]
fn approx_zero_handles_edge_lane_patterns() {
    assert!(Corners::ZERO.is_approx_zero(), "all-zero bytes");
    assert!(Corners::all(0.0).is_approx_zero(), "+0.0 lanes");
    assert!(
        Corners::all(-0.0).is_approx_zero(),
        "-0.0 lanes (sign bit set)"
    );
    assert!(Corners::all(EPS * 0.5).is_approx_zero(), "sub-EPS positive");
    assert!(
        !Corners::all(EPS * 10.0).is_approx_zero(),
        "10×EPS must NOT register as zero",
    );
    // One asymmetric lane above EPS must not be accepted because the other three are zero.
    assert!(
        !Corners::new(0.0, 0.0, 1.0, 0.0).is_approx_zero(),
        "single non-zero lane breaks zero contract",
    );
    // NaN bits sit in the exponent region (≥ 0x7C00), far above EPS: non-zero.
    assert!(
        !Corners(F16x4::from_lanes([f32::NAN; 4])).is_approx_zero(),
        "NaN lanes are not zero"
    );
}

/// The 4-tuple is lane order, as `Corners::new` and CSS `border-radius` read: top-left, then
/// clockwise.
#[test]
fn a_four_tuple_maps_to_lanes() {
    assert_eq!(Corners::from((1, 2, 3, 4)).to_array(), [1.0, 2.0, 3.0, 4.0]);
    assert_eq!(
        Corners::from((1, 2, 3, 4)),
        Corners::new(1.0, 2.0, 3.0, 4.0)
    );
}

/// The packed chrome-hash form: four f16 lanes, little-endian `tl | tr | br | bl`. 1.0 is `0x3c00`, 2.0 `0x4000`, so `(1, 2, 1, 2)` packs to `0x4000_3c00_4000_3c00`.
#[test]
fn as_u64_packs_the_four_lanes() {
    assert_eq!(
        Corners::new(1.0, 2.0, 1.0, 2.0).as_u64(),
        0x4000_3c00_4000_3c00
    );
    assert_eq!(Corners::ZERO.as_u64(), 0);
}

#[test]
fn convenience_ctors() {
    assert_eq!(Corners::top(4.0).to_array(), [4.0, 4.0, 0.0, 0.0]);
    assert_eq!(Corners::bottom(4.0).to_array(), [0.0, 0.0, 4.0, 4.0]);
    assert_eq!(Corners::left(4.0).to_array(), [4.0, 0.0, 0.0, 4.0]);
    assert_eq!(Corners::right(4.0).to_array(), [0.0, 4.0, 4.0, 0.0]);
}

/// A file radius is a non-negative f32 that rounds to a finite f16: 0 and 65519.99, which rounds to 65504, are the ends; -1, 65520, infinity and NaN fail with the rule.
#[test]
fn deserialize_rejects_a_radius_f16_cannot_hold() {
    for (input, valid) in [
        ("0.0", true),
        ("65504.0", true),
        ("-1.0", false),
        ("65519.99", true),
        ("65520.0", false),
        ("inf", false),
        ("NaN", false),
    ] {
        let parsed = ron::from_str::<Corners>(input);
        assert_eq!(parsed.is_ok(), valid, "{input}: {parsed:?}");
        if let Err(error) = parsed {
            assert!(
                error.to_string().contains(Corners::LANE_RULE),
                "{input}: {error}"
            );
        }
    }
}

/// Corners' own choices on the shared wire format: no pair writes as two nodes, the top pair
/// and the diagonals alike, and a 2-node array is refused, as CSS and `Spacing` read it two other
/// ways. A table names `tl`, `tr`, `br`, `bl`, and the corners it leaves out are square. Each text
/// reads back as the corners it was written from.
#[test]
fn corners_write_no_pair_and_name_each_corner() {
    for (corners, text) in [
        (Corners::top(4.0), "[4.0,4.0,0.0,0.0]"),
        (Corners::new(1.0, 2.0, 1.0, 2.0), "[1.0,2.0,1.0,2.0]"),
    ] {
        assert_eq!(ron_text(&corners), text);
        assert_eq!(from_ron::<Corners>(text), corners, "{text}");
    }
    let error = ron::from_str::<Corners>("[4.0,0.0]")
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("a number, a 1- or 4-node array, or a {tl, tr, br, bl} table")
            && error.contains("found 2 elements"),
        "{error}"
    );
    assert_eq!(
        from_ron::<Corners>("(tl: 1.0, tr: 2.0, br: 3.0, bl: 4.0)"),
        Corners::new(1.0, 2.0, 3.0, 4.0)
    );
    assert_eq!(from_ron::<Corners>("(tl: 4.0, tr: 4.0)"), Corners::top(4.0));
}
