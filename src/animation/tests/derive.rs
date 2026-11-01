//! `#[derive(Animatable)]`, read back field by field on probe structs.

use crate::primitives::math::animatable::Animatable;
use crate::primitives::paint::color::RgbaF32;
use glam::Vec2;
use palantir_anim_derive::Animatable;

/// Two animated fields, a lerp-only field and two snap fields.
#[derive(Clone, Copy, Debug, PartialEq, Animatable)]
struct Probe {
    x: f32,
    v: Vec2,
    #[animate(lerp_only)]
    size: f32,
    #[animate(snap)]
    label: u8,
    #[animate(snap)]
    mode: u8,
}

/// Every field snaps.
#[derive(Clone, Copy, Debug, PartialEq, Animatable)]
struct AllSnap {
    #[animate(snap)]
    a: u8,
    #[animate(snap)]
    b: u16,
}

/// One tolerance for the whole struct: `0.5`, a power of two, so its square divides exactly.
#[derive(Clone, Copy, Debug, PartialEq, Animatable)]
#[animate(settle_eps = 0.5)]
struct Scaled {
    a: f32,
    b: f32,
}

/// Two fields, each settling in its own unit.
#[derive(Clone, Copy, Debug, PartialEq, Animatable)]
struct Mixed {
    color: RgbaF32,
    scaled: Scaled,
    #[animate(snap)]
    label: u8,
}

/// Generic over the animated field.
#[derive(Clone, Copy, Debug, PartialEq, Animatable)]
struct Pair<T: Animatable + Copy> {
    first: T,
    second: T,
}

/// A hook that swaps the ends, so a lerp it runs in reads backwards.
#[derive(Clone, Copy, Debug, PartialEq, Animatable)]
#[animate(before_lerp = Self::swap)]
struct Hooked {
    x: f32,
}

impl Hooked {
    fn swap(a: &mut Self, b: &mut Self) {
        std::mem::swap(a, b);
    }
}

/// Each generated method against its hand-computed result. Animated fields use their own `Animatable`; a lerp-only field blends in `lerp` and keeps `self` in `sub`, `add` and `scale`; a snap field takes `a` below `t = 0.5` and `b` from it in `lerp`, keeps `self` in the arithmetic, and is `Default` in `zero`. Neither adds to the magnitude.
#[test]
fn derived_methods_split_animated_from_snapped_fields() {
    let a = Probe {
        x: 1.0,
        v: Vec2::new(2.0, 4.0),
        size: 10.0,
        label: 3,
        mode: 5,
    };
    let b = Probe {
        x: 5.0,
        v: Vec2::new(6.0, 0.0),
        size: 18.0,
        label: 7,
        mode: 9,
    };
    // 1 + (5 − 1)·¼ = 2; (2, 4) + ((6, 0) − (2, 4))·¼ = (3, 3); 10 + (18 − 10)·¼ = 12.
    let quarter = Probe {
        x: 2.0,
        v: Vec2::new(3.0, 3.0),
        size: 12.0,
        label: 3,
        mode: 5,
    };
    assert_eq!(Probe::lerp(a, b, 0.25), quarter);
    // 1 + 4·½ = 3; (4, 2); 14; the snap fields switch to `b` at ½.
    assert_eq!(
        Probe::lerp(a, b, 0.5),
        Probe {
            x: 3.0,
            v: Vec2::new(4.0, 2.0),
            size: 14.0,
            ..b
        },
    );
    assert_eq!(Probe::lerp(a, b, 0.0), a);
    assert_eq!(Probe::lerp(a, b, 1.0), b);
    assert_eq!(
        b.sub(a),
        Probe {
            x: 4.0,
            v: Vec2::new(4.0, -4.0),
            ..b
        },
    );
    assert_eq!(
        a.add(b),
        Probe {
            x: 6.0,
            v: Vec2::new(8.0, 4.0),
            ..a
        },
    );
    assert_eq!(
        a.scale(2.0),
        Probe {
            x: 2.0,
            v: Vec2::new(4.0, 8.0),
            ..a
        },
    );
    // 1² + 2² + 4² = 21; the lerp-only and snap fields add nothing.
    assert_eq!(a.magnitude_squared(), 21.0);
    assert_eq!(
        Probe::zero(),
        Probe {
            x: 0.0,
            v: Vec2::ZERO,
            size: 0.0,
            label: 0,
            mode: 0,
        },
    );
    assert_eq!(a.to_motion(), a);

    // The hook sees both ends past the exact ones: at ¼ it reads ¾ of the way, 1 + 4·¾ = 4.
    let (p, q) = (Hooked { x: 1.0 }, Hooked { x: 5.0 });
    assert_eq!(Hooked::lerp(p, q, 0.25), Hooked { x: 4.0 });
    assert_eq!(Hooked::lerp(p, q, 0.0), p);
    assert_eq!(Hooked::lerp(p, q, 1.0), q);

    // All snap: no arithmetic at all, and a zero magnitude.
    let (p, q) = (AllSnap { a: 1, b: 2 }, AllSnap { a: 3, b: 4 });
    assert_eq!(AllSnap::lerp(p, q, 0.49), p);
    assert_eq!(AllSnap::lerp(p, q, 0.5), q);
    assert_eq!(p.sub(q), p);
    assert_eq!(p.magnitude_squared(), 0.0);
    assert_eq!(AllSnap::zero(), AllSnap { a: 0, b: 0 });

    // Generic: the bound reaches each field. 3² + 4² = 25.
    let pair = Pair {
        first: 3.0_f32,
        second: 4.0,
    };
    assert_eq!(pair.magnitude_squared(), 25.0);
    assert_eq!(
        Pair::lerp(pair, Pair::zero(), 0.5),
        Pair {
            first: 1.5,
            second: 2.0,
        },
    );
}

/// The settle distance, compared against `1.0` by spring and duration snap. A struct naming a tolerance divides its magnitude by it: `(3² + 4²) / 0.5² = 100`. Otherwise it sums its fields' in their own units: a lane of `2^-14` is one `RgbaF32` tolerance (`2^-28 / 2^-28 = 1`), so 100 + 1 = 101; at the `f32` tolerance it would weigh `(6.10e-5 / 1e-4)² ≈ 0.373`. Snap fields weigh nothing.
#[test]
fn derived_settle_distance_measures_each_field_in_its_own_unit() {
    let scaled = Scaled { a: 3.0, b: 4.0 };
    assert_eq!(scaled.settle_distance_squared(), 100.0);
    let mixed = Mixed {
        color: RgbaF32::new(1.0 / 16384.0, 0.0, 0.0, 0.0),
        scaled,
        label: 7,
    };
    assert_eq!(mixed.settle_distance_squared(), 101.0);
    assert_eq!(AllSnap { a: 1, b: 2 }.settle_distance_squared(), 0.0);
}
