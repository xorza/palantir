use crate::effect::effect_registry::{EffectRegistry, ResolvedMaterial};
use crate::host::harness::Harness;
use crate::internals::panic_probe;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::identity::effect_id::EffectId;
use crate::primitives::math::domain;
use crate::primitives::math::lerp::Lerp;
use crate::primitives::packed::serde::LaneCodec;
use crate::primitives::paint::border::Border;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::paint::brush::gradient::conic_geometry::ConicGradient;
use crate::primitives::paint::brush::gradient::linear_geometry::LinearGradient;
use crate::primitives::paint::brush::gradient::radial_geometry::RadialGradient;
use crate::primitives::paint::brush::gradient::stops::Stop;
use crate::primitives::paint::brush::gradient::{Interpolation, Spread};
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::material::Material;
use crate::primitives::paint::material::lens::Lens;
use crate::primitives::paint::material::material_reach::MaterialReach;
use crate::primitives::paint::paint::Paint;
use crate::primitives::paint::shadow::Shadow;
use crate::primitives::reduction::{Reduction, Reductions};
use crate::shape::Shape;
use crate::shape::geometry::closed_geometry::ClosedGeometry;
use crate::shape::lower::LoweredPaint;
use crate::shape::lower::{brush, chrome, ramp};
use crate::shape::paint::closed_paint::ClosedPaint;
use crate::shape::paint::curve_ramp::CurveRamp;
use crate::shape::paint::lowered_brush::LoweredBrush;
use crate::shape::paint::lowered_material::{LoweredMaterial, MaterialRead};
use crate::shape::record::ShapeRecord;
use crate::shape::record_store::RecordStore;
use crate::shape::record_store::Resolve;
use crate::shape::record_store::recorded_gradients::GradientId;
use crate::shape::sealed::DrawWith;
use crate::widget_core::widget::Widget;
use glam::UVec2;
use glam::Vec2;
use std::collections::HashSet;

fn gradient_id(store: &mut RecordStore, value: &Brush) -> GradientId {
    match brush(store, value) {
        LoweredBrush::Gradient { id, .. } => id,
        LoweredBrush::Solid(_) => panic!("test gradient lowered to a solid brush"),
    }
}

fn with_corners(corners: Corners) -> Chrome {
    Chrome {
        corners,
        ..Chrome::new(RgbaF32::WHITE)
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
            Chrome::new(RgbaF32::srgba(1.0, f32::NAN, 1.0, 1.0)),
            domain::COLOR_RULE,
        ),
        (
            "stroke",
            Chrome::new(RgbaF32::WHITE).with_border(Border::new(RgbaF32::WHITE, f32::NAN)),
            domain::LENGTH_RULE,
        ),
        (
            "shadow",
            Chrome::new(RgbaF32::WHITE).with_shadow(Shadow {
                color: RgbaF32::WHITE,
                blur: f32::NAN,
                ..Shadow::default()
            }),
            domain::LENGTH_RULE,
        ),
        (
            "material",
            Chrome::new(RgbaF32::WHITE).with_material(Material::blur(f32::NAN)),
            domain::LENGTH_RULE,
        ),
        (
            "luminance ceiling",
            Chrome::new(RgbaF32::WHITE)
                .with_material(Material::blur(1.0).with_max_luminance(f32::NAN)),
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
        brush(&mut store, &LinearGradient::horizontal(stops).into())
            .hash_parts()
            .payload,
        brush(
            &mut store,
            &RadialGradient::new(center, center, stops).into(),
        )
        .hash_parts()
        .payload,
        brush(&mut store, &ConicGradient::new(center, 0.0, stops).into())
            .hash_parts()
            .payload,
        match ramp(&mut store, &ColorRamp::new(stops)) {
            CurveRamp::Interned { hash, .. } => hash.as_u64(),
            CurveRamp::None => panic!("a ramp always interns"),
        },
    ];
    let distinct: HashSet<u64> = hashes.iter().copied().collect();
    assert_eq!(distinct.len(), 4, "{hashes:?}");
}

/// A chrome's material lowers as it reads this record: through its effect over what is under it,
/// a backdrop, its fill as authored; as a plain box, its fallback in place of its fill and no
/// material, while transparency is reduced, the theme's surface for one that names none; or not
/// at all, where its effect's alternative paints nothing, its fill as authored. A rect shape
/// lowers the same way, its read resolved in the registry, where the stock frost is effect 2. A
/// fallback reaches the row's hash as its fill, and a capture's row hashes by none of it.
#[test]
fn a_material_lowers_as_it_reads() {
    let panel = RgbaF32::hex(0x204060);
    let color = panel.with_alpha8(184);
    let theme = RgbaF32::hex(0x101820).with_alpha8(200);
    let own = Chrome::new(color).with_material(Material::frosted(20.0).with_fallback(panel));
    let themed = Chrome::new(color).with_material(Material::frosted(20.0));
    let solid = |brush: LoweredBrush| match brush {
        LoweredBrush::Solid(color) => color,
        LoweredBrush::Gradient { .. } => panic!("a solid fill lowers to a solid brush"),
    };
    let id = EffectId(2);
    let mut store = RecordStore::default();
    let fallback = MaterialRead::Fallback { theme };
    for (chrome_, read, material, fill) in [
        (
            &own,
            Some(MaterialRead::Capture(ResolvedMaterial {
                id,
                reach: MaterialReach::NONE,
            })),
            Some(id),
            color,
        ),
        (&own, Some(fallback), None, panel),
        (&themed, Some(fallback), None, theme),
        (&own, None, None, color),
    ] {
        let row = chrome(&mut store, chrome_, Border::NONE, read);
        assert_eq!(
            row.material.map(LoweredMaterial::effect),
            material,
            "{read:?}"
        );
        assert_eq!(solid(row.fill), fill.to_f16(), "{read:?}");
    }

    let effects = EffectRegistry::default();
    let reduced = Reductions {
        transparency: Reduction::Reduced,
        ..Reductions::FULL
    };
    let rect = |store: &mut RecordStore, reductions| {
        let resolve = Resolve {
            effects: &effects,
            reductions,
            material_fallback: theme,
        };
        store.record(DrawWith::<Paint>::draw(&Shape::rect(), &own.paint), resolve)
    };
    for (reductions, material, fill) in
        [(Reductions::FULL, Some(id), color), (reduced, None, panel)]
    {
        let Some(ShapeRecord::Closed {
            geometry: ClosedGeometry::Box(_),
            paint:
                ClosedPaint::Fill(LoweredPaint {
                    fill: lowered,
                    material: worn,
                    ..
                }),
        }) = rect(&mut store, reductions)
        else {
            panic!("a rect lowers to a rect quad");
        };
        assert_eq!(worn.map(LoweredMaterial::effect), material);
        assert_eq!(solid(lowered), fill.to_f16());
    }

    // A fallback of nothing leaves nothing to paint but a border: no record without one.
    let bare =
        Paint::from(color).with_material(Material::frosted(20.0).with_fallback(Brush::TRANSPARENT));
    let resolve = Resolve {
        effects: &effects,
        reductions: reduced,
        material_fallback: theme,
    };
    let bordered = bare.clone().with_border(Border::new(RgbaF32::WHITE, 1.0));
    let rect_shape = Shape::rect();
    let draw = |paint| DrawWith::<Paint>::draw(&rect_shape, paint);
    assert!(store.record(draw(&bare), resolve).is_none());
    assert!(store.record(draw(&bordered), resolve).is_some());

    let recolored =
        Chrome::new(color).with_material(Material::frosted(20.0).with_fallback(RgbaF32::BLACK));
    let mut hash =
        |authored: &Chrome, read| chrome(&mut store, authored, Border::NONE, Some(read)).hash;
    assert_ne!(
        hash(&own, fallback),
        hash(&recolored, fallback),
        "the fallback it paints"
    );
    let capture = MaterialRead::Capture(ResolvedMaterial {
        id,
        reach: MaterialReach::NONE,
    });
    assert_eq!(
        hash(&own, capture),
        hash(&recolored, capture),
        "a fallback it does not paint"
    );
}

#[test]
fn chrome_lowering_keeps_an_authored_field() {
    let mut store = RecordStore::default();
    let sane = Corners::all(6.0);
    let kept = chrome(&mut store, &with_corners(sane), Border::NONE, None);
    assert_eq!(kept.corners, sane);
    assert_ne!(
        kept.hash,
        chrome(&mut store, &with_corners(Corners::ZERO), Border::NONE, None).hash,
        "corners must still reach the chrome hash",
    );
    let ring = Border::new(RgbaF32::WHITE, 2.0);
    let ringed = chrome(&mut store, &with_corners(sane), ring, None);
    assert!(ringed.ring, "a ring is flagged on the row");
    assert!(!kept.ring, "no ring unless one is passed");
    assert_ne!(
        ringed.hash, kept.hash,
        "the ring must reach the chrome hash"
    );
    // A transparent border paints nothing, but its width is the inset shadow's padding edge, so it is kept.
    let clear_border = with_corners(sane).with_border(Border::new(RgbaF32::TRANSPARENT, 4.0));
    let clear = chrome(&mut store, &clear_border, Border::NONE, None);
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
    let base = with_corners(sane)
        .with_border(Border::new(RgbaF32::WHITE, 1.0))
        .with_shadow(shadow);
    let with_shadow = |shadow| base.clone().with_shadow(shadow);
    let gradient: Brush = LinearGradient::horizontal([RgbaF32::BLACK, RgbaF32::WHITE]).into();
    let variants = [
        ("base", base.clone()),
        (
            "border color",
            base.clone().with_border(Border::new(RgbaF32::BLACK, 1.0)),
        ),
        ("fill color", base.clone().with_fill(RgbaF32::BLACK)),
        ("gradient fill", base.clone().with_fill(gradient)),
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
            base.clone().with_material(Material::blur(4.0)),
        ),
        (
            "backdrop frosted",
            base.clone().with_material(Material::frosted(4.0)),
        ),
        (
            "backdrop ceiling",
            base.clone()
                .with_material(Material::blur(4.0).with_max_luminance(0.1)),
        ),
        (
            "backdrop lens",
            base.clone()
                .with_material(Material::glass(Lens::WINDOW_GLASS)),
        ),
    ];
    let hashes: Vec<_> = variants
        .iter()
        .map(|(label, authored)| {
            let read = authored.paint.material.as_ref().map(|_| {
                MaterialRead::Capture(ResolvedMaterial {
                    id: EffectId(2),
                    reach: MaterialReach::NONE,
                })
            });
            (
                *label,
                chrome(&mut store, authored, Border::NONE, read).hash,
            )
        })
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
    let base = LinearGradient::new(0.25, [colors[0], colors[1]]);
    let first = gradient_id(&mut store, &Brush::Linear(base.clone()));
    assert_eq!(gradient_id(&mut store, &Brush::Linear(base.clone())), first);

    let changed_geometry = gradient_id(
        &mut store,
        &LinearGradient::new(0.75, [colors[0], colors[1]]).into(),
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
        &RadialGradient::centered([colors[0], colors[1]]).into(),
    );
    let conic = gradient_id(
        &mut store,
        &ConicGradient::centered([colors[0], colors[1]]).into(),
    );
    assert!(!mode_ids.contains(&radial));
    assert!(!mode_ids.contains(&conic));
    assert_ne!(radial, conic);
    assert_eq!(store.gradients.records.len(), 9);
}
