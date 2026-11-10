use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::domain::EPS;
use crate::primitives::math::float_hash::{FloatHash, canon_bits};
use glam::Vec2;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher as _;

fn finish_hash(write: impl FnOnce(&mut DefaultHasher)) -> u64 {
    let mut hasher = DefaultHasher::new();
    write(&mut hasher);
    hasher.finish()
}

#[test]
fn exact_hash_helpers_collapse_only_signed_zero() {
    let positive = Rect::new(0.0, 0.0, 0.0, 0.0);
    let negative = Rect::new(-0.0, -0.0, -0.0, -0.0);
    let sub_eps = Rect::new(EPS * 0.5, 0.0, 0.0, 0.0);

    assert_eq!(
        finish_hash(|h| positive.hash_eq(h)),
        finish_hash(|h| negative.hash_eq(h)),
    );
    assert_ne!(
        finish_hash(|h| positive.hash_eq(h)),
        finish_hash(|h| sub_eps.hash_eq(h)),
    );
}

#[test]
fn visual_hash_helpers_collapse_zero_noise_and_nan_payloads() {
    let zero = Rect::ZERO;
    let sub_eps = Rect::new(EPS * 0.5, -EPS * 0.5, EPS, -EPS);
    assert_eq!(
        finish_hash(|h| zero.hash_visual(h)),
        finish_hash(|h| sub_eps.hash_visual(h)),
    );

    let nan_a = f32::from_bits(0x7fc0_0001);
    let nan_b = f32::from_bits(0x7fc0_0002);
    assert_eq!(canon_bits(nan_a), canon_bits(nan_b));
    assert_eq!(
        finish_hash(|h| nan_a.hash_visual(h)),
        finish_hash(|h| nan_b.hash_visual(h)),
    );
}

/// Every `FloatHash` type hashes `0.0` and `-0.0` alike, as they compare equal (`Hash`/`Eq` agreement): via `hash_eq` for the two foreign types, `Hash` for the crate's own.
#[test]
fn signed_zeros_hash_alike_for_every_float_hash_type() {
    use crate::primitives::geometry::size::Size;
    use crate::primitives::layout::sizing::{SizeSpec, Sizing};
    use crate::primitives::layout::track::Track;
    use crate::primitives::paint::color::RgbaF32;
    use std::fmt::Debug;
    use std::hash::Hash;

    #[track_caller]
    fn agree<T: Hash + PartialEq + Debug>(positive: &T, negative: &T) {
        assert_eq!(positive, negative);
        assert_eq!(
            finish_hash(|h| positive.hash(h)),
            finish_hash(|h| negative.hash(h)),
            "{positive:?}",
        );
    }

    assert_eq!(
        finish_hash(|h| 0.0_f32.hash_eq(h)),
        finish_hash(|h| (-0.0_f32).hash_eq(h)),
        "f32",
    );
    assert_eq!(
        finish_hash(|h| Vec2::ZERO.hash_eq(h)),
        finish_hash(|h| Vec2::splat(-0.0).hash_eq(h)),
        "Vec2",
    );
    agree(&Size::new(0.0, 0.0), &Size::new(-0.0, -0.0));
    agree(
        &Rect::new(0.0, 0.0, 0.0, 0.0),
        &Rect::new(-0.0, -0.0, -0.0, -0.0),
    );
    agree(
        &RgbaF32::new(0.0, 0.0, 0.0, 0.0),
        &RgbaF32::new(-0.0, -0.0, -0.0, -0.0),
    );
    agree(&Sizing::fixed(0.0), &Sizing::fixed(-0.0));
    agree(
        &SizeSpec::new(Sizing::fixed(0.0), Sizing::HUG),
        &SizeSpec::new(Sizing::fixed(-0.0), Sizing::HUG),
    );
    agree(
        &Track::new(Sizing::fixed(0.0)).with_min(0.0),
        &Track::new(Sizing::fixed(-0.0)).with_min(-0.0),
    );
}

/// An option feeds its tag, then its value under the same tolerance: `None` and `Some(ZERO)` hash
/// apart, `Some` of two values within the zero tolerance hash alike, and the tag is the byte
/// written first.
#[test]
fn an_option_hashes_its_tag_then_its_value() {
    let sub_eps = Rect::new(EPS * 0.5, 0.0, 0.0, 0.0);
    let visual = |rect: Option<Rect>| finish_hash(|h| rect.hash_visual(h));
    assert_ne!(visual(None), visual(Some(Rect::ZERO)));
    assert_eq!(visual(Some(Rect::ZERO)), visual(Some(sub_eps)));
    assert_eq!(
        visual(Some(Rect::ZERO)),
        finish_hash(|h| {
            h.write_u8(1);
            Rect::ZERO.hash_visual(h);
        })
    );
    assert_eq!(visual(None), finish_hash(|h| h.write_u8(0)));
    let eq = |rect: Option<Rect>| finish_hash(|h| rect.hash_eq(h));
    assert_ne!(eq(Some(Rect::ZERO)), eq(Some(sub_eps)));
    assert_ne!(eq(None), eq(Some(Rect::ZERO)));
}
