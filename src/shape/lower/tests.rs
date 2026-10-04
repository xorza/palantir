use crate::primitives::geometry::corners::Corners;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::shadow::Shadow;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::lower::background;

use super::brush;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::brush::gradient::conic_geometry::ConicGradient;
use crate::primitives::paint::brush::gradient::linear_geometry::LinearGradient;
use crate::primitives::paint::brush::gradient::radial_geometry::RadialGradient;
use crate::primitives::paint::brush::gradient::stops::Stop;
use crate::primitives::paint::brush::gradient::{Interpolation, Spread};

use crate::scene::record_store::RecordStore;
use crate::scene::record_store::recorded_gradients::GradientId;
use crate::shape::paint::shape_brush::ShapeBrush;
use std::collections::HashSet;
use std::panic;

fn gradient_id(store: &mut RecordStore, value: &Brush) -> GradientId {
    match brush(store, value) {
        ShapeBrush::Gradient { id, .. } => id,
        ShapeBrush::Solid(_) => panic!("test gradient lowered to a solid brush"),
    }
}

/// A white fill with `corners`, the shape every chrome case here
/// varies one field of.
fn with_corners(corners: Corners) -> Background {
    Background {
        corners,
        ..Background::fill(RgbaF32::WHITE)
    }
}

/// The four ways a `Background` can carry a NaN, one per field.
///
/// All four are covered because no no-op predicate owns the question
/// for any of them: "the radius is NaN" is not a reason the
/// background paints nothing, and `is_approx_zero` reports NaN as
/// non-zero by design so a NaN cannot take the sharp-corner fast
/// path.
fn nan_backgrounds() -> [(&'static str, Background); 4] {
    [
        (
            "corners",
            with_corners(Corners::new(4.0, f32::NAN, 4.0, 4.0)),
        ),
        (
            "fill",
            Background::fill(RgbaF32::srgba(1.0, f32::NAN, 1.0, 1.0)),
        ),
        (
            "stroke",
            Background {
                border: Stroke::new(RgbaF32::WHITE, f32::NAN),
                ..Background::fill(RgbaF32::WHITE)
            },
        ),
        (
            "shadow",
            Background {
                shadow: Shadow {
                    color: RgbaF32::WHITE,
                    blur: f32::NAN,
                    ..Shadow::default()
                },
                ..Background::fill(RgbaF32::WHITE)
            },
        ),
    ]
}

/// The three gradient kinds hash apart even on identical stops and
/// geometry, because [`gradient_brush`] folds a discriminant byte in
/// before the geometry. Without it a linear and a radial over the
/// same two stops would share a content hash, and a brush swap
/// between them would raise no damage.
#[test]
fn the_three_gradient_kinds_hash_apart_on_identical_stops() {
    let mut store = RecordStore::default();
    let stops = [
        Stop::new(0.0, RgbaF32::BLACK),
        Stop::new(1.0, RgbaF32::WHITE),
    ];
    let centre = glam::Vec2::splat(0.5);
    let hashes = [
        brush(&mut store, &Brush::Linear(LinearGradient::new(0.0, stops)))
            .hash_parts()
            .payload,
        brush(
            &mut store,
            &Brush::Radial(RadialGradient::new(centre, centre, stops)),
        )
        .hash_parts()
        .payload,
        brush(
            &mut store,
            &Brush::Conic(ConicGradient::new(centre, 0.0, stops)),
        )
        .hash_parts()
        .payload,
    ];
    let distinct: HashSet<u64> = hashes.iter().copied().collect();
    assert_eq!(distinct.len(), 3, "{hashes:?}");
}

/// A sane background reaches the row unchanged, and every field it
/// carries reaches the hash — which is what makes the sanitizing
/// below a change of behaviour rather than a no-op.
#[test]
fn background_lowering_keeps_an_authored_field() {
    let mut store = RecordStore::default();
    let sane = Corners::all(6.0);
    let kept = background(&mut store, &with_corners(sane), Stroke::NONE);
    assert_eq!(kept.corners, sane);
    assert_ne!(
        kept.hash,
        background(&mut store, &with_corners(Corners::ZERO), Stroke::NONE).hash,
        "corners must still reach the chrome hash",
    );
    let ring = Stroke::new(RgbaF32::WHITE, 2.0);
    let ringed = background(&mut store, &with_corners(sane), ring);
    assert!(ringed.ring, "a ring is flagged on the row");
    assert!(!kept.ring, "no ring unless one is passed");
    assert_ne!(
        ringed.hash, kept.hash,
        "the ring must reach the chrome hash"
    );
    // A transparent border paints nothing, but its width is the padding
    // edge an inset shadow paints inside, so the row and its hash keep it.
    let clear_border = Background {
        border: Stroke::new(RgbaF32::TRANSPARENT, 4.0),
        ..with_corners(sane)
    };
    let clear = background(&mut store, &clear_border, Stroke::NONE);
    assert_eq!(clear.border.width, 4.0);
    assert!(clear.border.is_noop(), "it still paints no stroke");
    assert_ne!(
        clear.hash, kept.hash,
        "the border's width must reach the chrome hash"
    );

    // Every other field, one at a time off the same base: each must
    // land on a hash of its own.
    let shadow = Shadow {
        color: RgbaF32::BLACK,
        offset: glam::Vec2::new(1.0, 2.0),
        blur: 3.0,
        spread: 1.0,
        inset: false,
    };
    let base = Background {
        border: Stroke::new(RgbaF32::WHITE, 1.0),
        shadow,
        ..with_corners(sane)
    };
    let with_shadow = |shadow| Background {
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
            "border colour",
            Background {
                border: Stroke::new(RgbaF32::BLACK, 1.0),
                ..base.clone()
            },
        ),
        (
            "fill colour",
            Background {
                fill: RgbaF32::BLACK.into(),
                ..base.clone()
            },
        ),
        (
            "gradient fill",
            Background {
                fill: gradient,
                ..base.clone()
            },
        ),
        (
            "shadow colour",
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
        .map(|(label, bg)| (*label, background(&mut store, bg, Stroke::NONE).hash))
        .collect();
    for (i, (a, ha)) in hashes.iter().enumerate() {
        for (b, hb) in &hashes[i + 1..] {
            assert_ne!(ha, hb, "`{a}` and `{b}` must hash apart");
        }
    }
}

/// Chrome is the paint path `Shapes::add` never sees, so `background`
/// is its NaN gate — and it sanitizes where the shape path drops,
/// because `chrome_table` keeps a row for `ClipMode::Rounded` even
/// when the paint is no-op. A dropped background would fix the fill
/// and leave the stencil mask reading the NaN.
///
/// **The claim is one both profiles keep: a NaN never reaches the
/// row.** A debug build says so by asserting, a release build by
/// falling each field back to what its NaN already meant, and the
/// `catch_unwind` accepts either — the same shape
/// `the_nan_gate_drops_every_shape_kind` pins the shape path with.
#[test]
fn a_nan_background_field_never_reaches_the_row() {
    let mut store = RecordStore::default();
    for (label, authored) in nan_backgrounds() {
        let Ok(row) = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            background(&mut store, &authored, Stroke::NONE)
        })) else {
            // The gate asserted, which is the loudest form of "did
            // not reach the row".
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

    // A radius falls back to *no rounding* specifically, not merely
    // to something finite: that is what leaves a `ClipMode::Rounded`
    // stencil readable rather than clipping to a shape nobody chose.
    if let Ok(row) = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        background(
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
