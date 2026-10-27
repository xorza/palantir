use crate::internals::panic_probe;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::layout::track::Track;
use crate::primitives::math::domain;

/// A track starts unclamped, `0..=+inf`, with the sizing it was built from; the clamps read back
/// as set in either order.
#[test]
fn bounds_accept_valid_ranges_in_either_order() {
    const MIN_THEN_MAX: Track = Track::FILL.with_min(10.0).with_max(20.0);
    const MAX_THEN_MIN: Track = Track::FILL.with_max(20.0).with_min(10.0);
    const PINNED: Track = Track::fixed(5.0).with_min(5.0).with_max(5.0);

    assert_eq!(Track::HUG.sizing(), Sizing::HUG);
    assert_eq!(Track::HUG.min(), 0.0);
    assert_eq!(Track::HUG.max(), f32::INFINITY);
    assert_eq!(MIN_THEN_MAX, MAX_THEN_MIN);
    assert_eq!(MIN_THEN_MAX.sizing(), Sizing::FILL);
    assert_eq!(MIN_THEN_MAX.min(), 10.0);
    assert_eq!(MIN_THEN_MAX.max(), 20.0);
    assert_eq!(PINNED.sizing(), Sizing::fixed(5.0));
    assert_eq!(PINNED.min(), 5.0);
    assert_eq!(PINNED.max(), 5.0);
}

/// A minimum is a length and a maximum an extent, so each panics with its kind's rule. Order is coerced: the minimum wins, raising a lower maximum in either setter order.
#[test]
fn bounds_validate_their_kinds_and_coerce_their_order() {
    type Case = (&'static str, fn() -> Track);

    let cases: &[Case] = &[
        (domain::LENGTH_RULE, || Track::HUG.with_min(-1.0)),
        (domain::LENGTH_RULE, || Track::HUG.with_min(f32::NAN)),
        (domain::LENGTH_RULE, || Track::HUG.with_min(f32::INFINITY)),
        (domain::EXTENT_RULE, || Track::HUG.with_max(-1.0)),
        (domain::EXTENT_RULE, || {
            Track::HUG.with_max(f32::NEG_INFINITY)
        }),
        (domain::EXTENT_RULE, || Track::HUG.with_max(f32::NAN)),
    ];
    for &(expected, build) in cases {
        panic_probe::assert_panics_with(expected, build);
    }

    assert_eq!(Track::HUG.with_max(f32::INFINITY).max(), f32::INFINITY);
    let raised = Track::HUG.with_max(10.0).with_min(11.0);
    assert_eq!((raised.min(), raised.max()), (11.0, 11.0), "max then min");
    let raised = Track::HUG.with_min(11.0).with_max(10.0);
    assert_eq!((raised.min(), raised.max()), (11.0, 11.0), "min then max");
}
