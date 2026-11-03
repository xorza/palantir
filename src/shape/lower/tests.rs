use crate::host::harness::Harness;
use crate::internals::panic_probe;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::math::domain;
use crate::primitives::math::lerp::Lerp;
use crate::primitives::packed::serde::LaneCodec;
use crate::primitives::paint::backdrop_filter::{BackdropFilter, Lens};
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::paint::brush::gradient::conic_geometry::ConicGradient;
use crate::primitives::paint::brush::gradient::linear_geometry::LinearGradient;
use crate::primitives::paint::brush::gradient::radial_geometry::RadialGradient;
use crate::primitives::paint::brush::gradient::stops::Stop;
use crate::primitives::paint::brush::gradient::{Interpolation, Spread};
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::shadow::Shadow;
use crate::primitives::paint::stroke::Stroke;
use crate::primitives::reduce_preference::Reduction;
use crate::shape::Shape;
use crate::shape::lower::{brush, chrome, ramp};
use crate::shape::paint::curve_ramp::CurveRamp;
use crate::shape::paint::quad_shape::QuadShape;
use crate::shape::paint::shape_brush::ShapeBrush;
use crate::shape::record::ShapeRecord;
use crate::shape::record_store::RecordStore;
use crate::shape::record_store::recorded_gradients::GradientId;
use crate::shape::sealed::LowerShape;
use crate::widget_core::widget::Widget;
use glam::UVec2;
use glam::Vec2;
use std::collections::HashSet;

fn gradient_id(store: &mut RecordStore, value: &Brush) -> GradientId {
    match brush(store, value) {
        ShapeBrush::Gradient { id, .. } => id,
        ShapeBrush::Solid(_) => panic!("test gradient lowered to a solid brush"),
    }
}

fn with_corners(corners: Corners) -> Chrome {
    Chrome {
        corners,
        ..Chrome::fill(RgbaF32::WHITE)
    }
}

/// The ways a `Chrome` can carry a NaN, one per field, each with the rule its check refuses it by;
/// no no-op predicate owns the question.
fn nan_chromes() -> [(&'static str, Chrome, &'static str); 6] {
    [
        (
            "corners",
            // The constructors refuse a NaN radius; a blend at a NaN weight still makes one, as
            // the floor its blend is coerced to keeps a NaN.
            with_corners(Corners::lerp(&Corners::ZERO, &Corners::all(4.0), f32::NAN)),
            <Corners as LaneCodec>::LANE_RULE,
        ),
        (
            "fill",
            Chrome::fill(RgbaF32::srgba(1.0, f32::NAN, 1.0, 1.0)),
            domain::COLOR_RULE,
        ),
        (
            "stroke",
            Chrome {
                border: Stroke::new(RgbaF32::WHITE, f32::NAN),
                ..Chrome::fill(RgbaF32::WHITE)
            },
            domain::LENGTH_RULE,
        ),
        (
            "shadow",
            Chrome {
                shadow: Shadow {
                    color: RgbaF32::WHITE,
                    blur: f32::NAN,
                    ..Shadow::default()
                },
                ..Chrome::fill(RgbaF32::WHITE)
            },
            domain::LENGTH_RULE,
        ),
        (
            "backdrop filter",
            Chrome::fill(RgbaF32::WHITE).with_backdrop_filter(BackdropFilter::blur(f32::NAN)),
            domain::LENGTH_RULE,
        ),
        (
            "luminance ceiling",
            Chrome::fill(RgbaF32::WHITE)
                .with_backdrop_filter(BackdropFilter::blur(1.0).with_max_luminance(f32::NAN)),
            domain::FACTOR_RULE,
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
    let center = Vec2::splat(0.5);
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

/// A sane chrome reaches the row unchanged and every field reaches the hash.
/// Under reduced transparency a pane lowers as a solid fill with no filter: its fill made opaque,
/// or its fallback; a chrome with no filter, and a pane under full transparency, lower as they
/// are. A rect shape lowers the same way.
#[test]
fn reduced_transparency_lowers_a_pane_as_a_solid_fill() {
    let tint = RgbaF32::hex(0x204060).with_alpha8(184);
    let frosted = Chrome::fill(tint).with_backdrop_filter(BackdropFilter::frosted(20.0));
    let fallback = RgbaF32::hex(0x808080);
    let glass = Chrome::fill(RgbaF32::TRANSPARENT)
        .with_backdrop_filter(BackdropFilter::frosted(20.0).with_fallback(fallback));
    let plain = Chrome::fill(tint);
    let solid = |brush: ShapeBrush| match brush {
        ShapeBrush::Solid(color) => color,
        ShapeBrush::Gradient { .. } => panic!("a solid pane lowers to a solid brush"),
    };

    let mut full = RecordStore::default();
    let row = chrome(&mut full, &frosted, Stroke::NONE);
    assert!(row.backdrop.is_some(), "full transparency keeps the filter");
    assert_eq!(solid(row.fill), tint.to_f16());

    let mut reduced = RecordStore::default();
    reduced.transparency = Reduction::Reduced;
    for (label, pane, want) in [
        ("tinted", &frosted, RgbaF32::hex(0x204060)),
        ("fallback", &glass, fallback),
        ("no filter", &plain, tint),
    ] {
        let row = chrome(&mut reduced, pane, Stroke::NONE);
        assert!(
            row.backdrop.is_none(),
            "{label}: no filter under reduced transparency"
        );
        assert_eq!(solid(row.fill), want.to_f16(), "{label}");
    }

    let rect = |store: &mut RecordStore| {
        Shape::rect()
            .fill(tint)
            .backdrop_filter(BackdropFilter::frosted(20.0))
            .lower(store)
    };
    let ShapeRecord::Quad(QuadShape::Rect { fill, backdrop, .. }) = rect(&mut reduced) else {
        panic!("a rect lowers to a rect quad");
    };
    assert!(backdrop.is_none());
    assert_eq!(solid(fill), RgbaF32::hex(0x204060).to_f16());
    let ShapeRecord::Quad(QuadShape::Rect { backdrop, .. }) = rect(&mut full) else {
        panic!("a rect lowers to a rect quad");
    };
    assert!(backdrop.is_some());
}

#[test]
fn chrome_lowering_keeps_an_authored_field() {
    let mut store = RecordStore::default();
    let sane = Corners::all(6.0);
    let kept = chrome(&mut store, &with_corners(sane), Stroke::NONE);
    assert_eq!(kept.corners, sane);
    assert_ne!(
        kept.hash,
        chrome(&mut store, &with_corners(Corners::ZERO), Stroke::NONE).hash,
        "corners must still reach the chrome hash",
    );
    let ring = Stroke::new(RgbaF32::WHITE, 2.0);
    let ringed = chrome(&mut store, &with_corners(sane), ring);
    assert!(ringed.ring, "a ring is flagged on the row");
    assert!(!kept.ring, "no ring unless one is passed");
    assert_ne!(
        ringed.hash, kept.hash,
        "the ring must reach the chrome hash"
    );
    // A transparent border paints nothing, but its width is the inset shadow's padding edge, so it is kept.
    let clear_border = Chrome {
        border: Stroke::new(RgbaF32::TRANSPARENT, 4.0),
        ..with_corners(sane)
    };
    let clear = chrome(&mut store, &clear_border, Stroke::NONE);
    assert_eq!(clear.border.width, 4.0);
    assert!(clear.border.is_noop(), "it still paints no stroke");
    assert_ne!(
        clear.hash, kept.hash,
        "the border's width must reach the chrome hash"
    );

    let shadow = Shadow {
        color: RgbaF32::BLACK,
        offset: Vec2::new(1.0, 2.0),
        blur: 3.0,
        spread: 1.0,
        inset: false,
    };
    let base = Chrome {
        border: Stroke::new(RgbaF32::WHITE, 1.0),
        shadow,
        ..with_corners(sane)
    };
    let with_shadow = |shadow| Chrome {
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
            Chrome {
                border: Stroke::new(RgbaF32::BLACK, 1.0),
                ..base.clone()
            },
        ),
        (
            "fill color",
            Chrome {
                fill: RgbaF32::BLACK.into(),
                ..base.clone()
            },
        ),
        (
            "gradient fill",
            Chrome {
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
                offset: Vec2::new(2.0, 1.0),
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
        (
            "backdrop blur",
            base.clone().with_backdrop_filter(BackdropFilter::blur(4.0)),
        ),
        (
            "backdrop frosted",
            base.clone()
                .with_backdrop_filter(BackdropFilter::frosted(4.0)),
        ),
        (
            "backdrop ceiling",
            base.clone()
                .with_backdrop_filter(BackdropFilter::blur(4.0).with_max_luminance(0.1)),
        ),
        (
            "backdrop lens",
            base.clone()
                .with_backdrop_filter(BackdropFilter::glass(Lens::WINDOW_GLASS)),
        ),
    ];
    let hashes: Vec<_> = variants
        .iter()
        .map(|(label, authored)| (*label, chrome(&mut store, authored, Stroke::NONE).hash))
        .collect();
    for (i, (a, ha)) in hashes.iter().enumerate() {
        for (b, hb) in &hashes[i + 1..] {
            assert_ne!(ha, hb, "`{a}` and `{b}` must hash apart");
        }
    }
}

/// `Widget::record` is the door every chrome passes, a widget built outside the crate's included:
/// a NaN in any field panics there, by its field's rule, rather than drawing quietly wrong.
#[test]
fn a_nan_chrome_field_panics_at_the_record_door() {
    for (_, authored, rule) in nan_chromes() {
        let mut h = Harness::mono(UVec2::new(40, 40));
        panic_probe::assert_panics_with(rule, || {
            h.frame(|ui| {
                Widget::leaf().record(ui, Some(&authored), |_| {});
            });
        });
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
        &Brush::Radial(RadialGradient::two_stop(
            Vec2::splat(0.5),
            Vec2::splat(0.5),
            colors[0],
            colors[1],
        )),
    );
    let conic = gradient_id(
        &mut store,
        &Brush::Conic(ConicGradient::two_stop(
            Vec2::splat(0.5),
            0.0,
            colors[0],
            colors[1],
        )),
    );
    assert!(!mode_ids.contains(&radial));
    assert!(!mode_ids.contains(&conic));
    assert_ne!(radial, conic);
    assert_eq!(store.gradients.records.len(), 9);
}
