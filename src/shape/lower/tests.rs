use crate::primitives::geometry::corners::Corners;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::bg::Bg;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::paint::brush::gradient::conic_geometry::ConicGradient;
use crate::primitives::paint::brush::gradient::linear_geometry::LinearGradient;
use crate::primitives::paint::brush::gradient::radial_geometry::RadialGradient;
use crate::primitives::paint::brush::gradient::stops::Stop;
use crate::primitives::paint::brush::gradient::{Interpolation, Spread};
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::shadow::Shadow;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::lower::{bg, brush, ramp};
use crate::shape::paint::curve_ramp::CurveRamp;
use crate::shape::paint::shape_brush::ShapeBrush;
use crate::shape::record_store::RecordStore;
use crate::shape::record_store::recorded_gradients::GradientId;
use std::collections::HashSet;
use std::panic;

fn gradient_id(store: &mut RecordStore, value: &Brush) -> GradientId {
    match brush(store, value) {
        ShapeBrush::Gradient { id, .. } => id,
        ShapeBrush::Solid(_) => panic!("test gradient lowered to a solid brush"),
    }
}

fn with_corners(corners: Corners) -> Bg {
    Bg {
        corners,
        ..Bg::fill(RgbaF32::WHITE)
    }
}

/// The four ways a `Bg` can carry a NaN, one per field; no no-op predicate owns the question.
fn nan_bgs() -> [(&'static str, Bg); 4] {
    [
        (
            "corners",
            with_corners(Corners::new(4.0, f32::NAN, 4.0, 4.0)),
        ),
        ("fill", Bg::fill(RgbaF32::srgba(1.0, f32::NAN, 1.0, 1.0))),
        (
            "stroke",
            Bg {
                border: Stroke::new(RgbaF32::WHITE, f32::NAN),
                ..Bg::fill(RgbaF32::WHITE)
            },
        ),
        (
            "shadow",
            Bg {
                shadow: Shadow {
                    color: RgbaF32::WHITE,
                    blur: f32::NAN,
                    ..Shadow::default()
                },
                ..Bg::fill(RgbaF32::WHITE)
            },
        ),
    ]
}

/// The three gradient kinds and a curve ramp hash apart on identical stops: each content hash starts with its fill kind's word.
#[test]
fn the_three_gradient_kinds_and_a_ramp_hash_apart_on_identical_stops() {
    let mut store = RecordStore::default();
    let stops = [
        Stop::new(0.0, RgbaF32::BLACK),
        Stop::new(1.0, RgbaF32::WHITE),
    ];
    let center = glam::Vec2::splat(0.5);
    let hashes = [
        brush(&mut store, &Brush::Linear(LinearGradient::new(0.0, stops)))
            .hash_parts()
            .payload,
        brush(
            &mut store,
            &Brush::Radial(RadialGradient::new(center, center, stops)),
        )
        .hash_parts()
        .payload,
        brush(
            &mut store,
            &Brush::Conic(ConicGradient::new(center, 0.0, stops)),
        )
        .hash_parts()
        .payload,
        match ramp(&mut store, &ColorRamp::new(stops)) {
            CurveRamp::Interned { hash, .. } => hash,
            CurveRamp::None => panic!("a ramp always interns"),
        },
    ];
    let distinct: HashSet<u64> = hashes.iter().copied().collect();
    assert_eq!(distinct.len(), 4, "{hashes:?}");
}

/// A sane background reaches the row unchanged and every field reaches the hash.
#[test]
fn background_lowering_keeps_an_authored_field() {
    let mut store = RecordStore::default();
    let sane = Corners::all(6.0);
    let kept = bg(&mut store, &with_corners(sane), Stroke::NONE);
    assert_eq!(kept.corners, sane);
    assert_ne!(
        kept.hash,
        bg(&mut store, &with_corners(Corners::ZERO), Stroke::NONE).hash,
        "corners must still reach the chrome hash",
    );
    let ring = Stroke::new(RgbaF32::WHITE, 2.0);
    let ringed = bg(&mut store, &with_corners(sane), ring);
    assert!(ringed.ring, "a ring is flagged on the row");
    assert!(!kept.ring, "no ring unless one is passed");
    assert_ne!(
        ringed.hash, kept.hash,
        "the ring must reach the chrome hash"
    );
    // A transparent border paints nothing, but its width is the inset shadow's padding edge, so it is kept.
    let clear_border = Bg {
        border: Stroke::new(RgbaF32::TRANSPARENT, 4.0),
        ..with_corners(sane)
    };
    let clear = bg(&mut store, &clear_border, Stroke::NONE);
    assert_eq!(clear.border.width, 4.0);
    assert!(clear.border.is_noop(), "it still paints no stroke");
    assert_ne!(
        clear.hash, kept.hash,
        "the border's width must reach the chrome hash"
    );

    let shadow = Shadow {
        color: RgbaF32::BLACK,
        offset: glam::Vec2::new(1.0, 2.0),
        blur: 3.0,
        spread: 1.0,
        inset: false,
    };
    let base = Bg {
        border: Stroke::new(RgbaF32::WHITE, 1.0),
        shadow,
        ..with_corners(sane)
    };
    let with_shadow = |shadow| Bg {
        shadow,
        ..base.clone()
    };
    let gradient = Brush::Linear(LinearGradient::new(
        0.0,
        [
            Stop::new(0.0, RgbaF32::BLACK),
            Stop::new(1.0, RgbaF32::WHITE),
        ],
    ));
    let variants = [
        ("base", base.clone()),
        (
            "border color",
            Bg {
                border: Stroke::new(RgbaF32::BLACK, 1.0),
                ..base.clone()
            },
        ),
        (
            "fill color",
            Bg {
                fill: RgbaF32::BLACK.into(),
                ..base.clone()
            },
        ),
        (
            "gradient fill",
            Bg {
                fill: gradient,
                ..base.clone()
            },
        ),
        (
            "shadow color",
            with_shadow(Shadow {
                color: RgbaF32::WHITE,
                ..shadow
            }),
        ),
        (
            "shadow offset",
            with_shadow(Shadow {
                offset: glam::Vec2::new(2.0, 1.0),
                ..shadow
            }),
        ),
        (
            "shadow blur",
            with_shadow(Shadow {
                blur: 4.0,
                ..shadow
            }),
        ),
        (
            "shadow spread",
            with_shadow(Shadow {
                spread: 2.0,
                ..shadow
            }),
        ),
        (
            "shadow inset",
            with_shadow(Shadow {
                inset: true,
                ..shadow
            }),
        ),
    ];
    let hashes: Vec<_> = variants
        .iter()
        .map(|(label, authored)| (*label, bg(&mut store, authored, Stroke::NONE).hash))
        .collect();
    for (i, (a, ha)) in hashes.iter().enumerate() {
        for (b, hb) in &hashes[i + 1..] {
            assert_ne!(ha, hb, "`{a}` and `{b}` must hash apart");
        }
    }
}

/// `bg` is the NaN gate for chrome, which `Shapes::add` never sees. It sanitizes rather than drops,
/// since `chrome_table` keeps a row for `ClipMode::Rounded` even when the paint is no-op. A NaN never reaches
/// the row: debug asserts, release falls each field back, and `catch_unwind` accepts either.
#[test]
fn a_nan_background_field_never_reaches_the_row() {
    let mut store = RecordStore::default();
    for (label, authored) in nan_bgs() {
        let Ok(row) = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            bg(&mut store, &authored, Stroke::NONE)
        })) else {
            continue;
        };
        assert!(
            !row.corners.has_nan()
                && !row.border.has_nan()
                && !row.shadow.has_nan()
                && !row.fill.has_nan(),
            "a NaN {label} must not survive lowering",
        );
    }

    // A radius falls back to no rounding so a `ClipMode::Rounded` stencil never clips to a shape nobody chose.
    if let Ok(row) = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        bg(
            &mut store,
            &with_corners(Corners::new(4.0, f32::NAN, 4.0, 4.0)),
            Stroke::NONE,
        )
    })) {
        assert!(row.corners.is_approx_zero(), "a radius collapses to none");
    }
}

#[test]
fn gradient_interning_identity_covers_geometry_kind_spread_and_interpolation() {
    let mut store = RecordStore::default();
    let colors = [RgbaF32::hex(0x1a1a2e), RgbaF32::hex(0x4c5cdb)];
    let base = LinearGradient::two_stop(0.25, colors[0], colors[1]);
    let first = gradient_id(&mut store, &Brush::Linear(base.clone()));
    assert_eq!(gradient_id(&mut store, &Brush::Linear(base.clone())), first);

    let changed_geometry = gradient_id(
        &mut store,
        &Brush::Linear(LinearGradient::two_stop(0.75, colors[0], colors[1])),
    );
    assert_ne!(changed_geometry, first);

    let mut mode_ids = HashSet::new();
    for spread in [Spread::Pad, Spread::Repeat, Spread::Reflect] {
        for interpolation in [Interpolation::Oklab, Interpolation::Linear] {
            let id = gradient_id(
                &mut store,
                &Brush::Linear(
                    base.clone()
                        .with_spread(spread)
                        .with_interpolation(interpolation),
                ),
            );
            assert!(
                mode_ids.insert(id),
                "spread/interpolation pair reused another pair's gradient id",
            );
        }
    }
    assert_eq!(mode_ids.len(), 6);

    let radial = gradient_id(
        &mut store,
        &Brush::Radial(RadialGradient::two_stop(colors[0], colors[1])),
    );
    let conic = gradient_id(
        &mut store,
        &Brush::Conic(ConicGradient::two_stop(colors[0], colors[1])),
    );
    assert!(!mode_ids.contains(&radial));
    assert!(!mode_ids.contains(&conic));
    assert_ne!(radial, conic);
    assert_eq!(store.gradients.records.len(), 9);
}
