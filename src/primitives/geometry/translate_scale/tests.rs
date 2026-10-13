use crate::internals::panic_probe;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::math::domain;
use crate::primitives::math::domain::EPS;
use glam::Vec2;

/// A transform is the identity when its bits are, or each part is within
/// `EPS` of it. `-0.0` and lerp drift take the `EPS` fallback; one pixel or
/// a 1.5 scale is past it.
#[test]
fn is_identity_within_eps() {
    let cases = [
        ("IDENTITY", TranslateScale::IDENTITY, true),
        ("new zero, 1", TranslateScale::new(Vec2::ZERO, 1.0), true),
        (
            "negative zero",
            TranslateScale::new(Vec2::new(-0.0, -0.0), 1.0),
            true,
        ),
        (
            "half-EPS drift",
            TranslateScale::new(Vec2::splat(EPS * 0.5), 1.0 + EPS * 0.5),
            true,
        ),
        (
            "one pixel",
            TranslateScale::from_translation(Vec2::new(1.0, 0.0)),
            false,
        ),
        ("scale 1.5", TranslateScale::from_scale(1.5), false),
    ];
    for (label, t, identity) in cases {
        assert_eq!(t.is_identity(), identity, "{label}");
    }
}

/// The door a caller builds a transform at, screened in every build.
#[test]
fn construction_rejects_non_finite_translation_and_non_positive_or_non_finite_scale() {
    let invalid_translations = [
        Vec2::new(f32::NAN, 0.0),
        Vec2::new(f32::INFINITY, 0.0),
        Vec2::new(f32::NEG_INFINITY, 0.0),
        Vec2::new(0.0, f32::NAN),
        Vec2::new(0.0, f32::INFINITY),
        Vec2::new(0.0, f32::NEG_INFINITY),
    ];
    for translation in invalid_translations {
        panic_probe::assert_panics_with(domain::OFFSET_RULE, || {
            TranslateScale::new(translation, 1.0)
        });
    }

    for scale in [0.0, -0.0, -1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        panic_probe::assert_panics_with(domain::POSITIVE_RULE, || {
            TranslateScale::new(Vec2::ZERO, scale)
        });
    }
}

/// The type's own arithmetic is held to the same contract, in debug only:
/// `from_parts` is where `compose` lands, per node per frame. Overflow is
/// how a derived transform breaks.
#[cfg(debug_assertions)]
#[test]
fn derived_transforms_reject_the_overflow_their_arithmetic_produces() {
    const TRANSLATION: &str = domain::OFFSET_RULE;
    const SCALE: &str = domain::POSITIVE_RULE;
    // Pivot arithmetic that overflows translation.
    panic_probe::assert_panics_with(TRANSLATION, || {
        TranslateScale::from_scale(f32::MAX).anchored_at(Vec2::splat(f32::MAX))
    });
    // Composition that overflows scale.
    panic_probe::assert_panics_with(SCALE, || {
        TranslateScale::from_scale(f32::MAX).compose(TranslateScale::from_scale(2.0))
    });
    // Composition that underflows scale to zero.
    panic_probe::assert_panics_with(SCALE, || {
        TranslateScale::from_scale(f32::from_bits(1)).compose(TranslateScale::from_scale(0.5))
    });
    // Composition that overflows translation.
    panic_probe::assert_panics_with(TRANSLATION, || {
        let transform = TranslateScale::from_translation(Vec2::splat(f32::MAX));
        transform.compose(transform)
    });
}

#[test]
fn composition_rect_application_and_inverse_vector_agree_exactly() {
    let parent = TranslateScale::new(Vec2::new(3.0, 5.0), 2.0);
    let child = TranslateScale::new(Vec2::new(7.0, 11.0), 4.0);
    let composed = parent.compose(child);

    assert_eq!(composed.translation, Vec2::new(17.0, 27.0));
    assert_eq!(composed.scale, 8.0);
    let point = Vec2::new(2.0, 3.0);
    assert_eq!(
        composed.apply_point(point),
        parent.apply_point(child.apply_point(point))
    );
    assert_eq!(
        composed.apply_rect(Rect::new(-2.0, 3.0, 4.0, 5.0)),
        Rect::new(1.0, 51.0, 32.0, 40.0)
    );
    assert_eq!(
        composed.inverse_vector(Vec2::new(24.0, -40.0)),
        Vec2::new(3.0, -5.0)
    );
}
