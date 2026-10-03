//! `#[derive(Animatable)]`, read back field by field on probe structs.

use crate::animation::animatable::Animatable;
use crate::primitives::paint::color::RgbaF32;
use glam::Vec2;
use palantir_anim_derive::Animatable;

/// Two animated fields and one each of the two snap spellings.
#[derive(Clone, Copy, Debug, PartialEq, Animatable)]
struct Probe {
    x: f32,
    v: Vec2,
    #[animate(snap)]
    label: u8,
    #[animate(skip)]
    mode: u8,
}

/// Every field snaps.
#[derive(Clone, Copy, Debug, PartialEq, Animatable)]
struct AllSnap {
    #[animate(snap)]
    a: u8,
    #[animate(skip)]
    b: u16,
}

/// One tolerance for the whole struct: `0.5`, a power of two, so its
/// square divides exactly.
#[derive(Clone, Copy, Debug, PartialEq, Animatable)]
#[animate(settle_eps = 0.5)]
struct Scaled {
    a: f32,
    b: f32,
}

/// Two fields, each settling in its own unit.
#[derive(Clone, Copy, Debug, PartialEq, Animatable)]
struct Mixed {
    colour: RgbaF32,
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

/// Each generated method against its hand-computed result. Animated
/// fields go through their own `Animatable`; a snap field, under either
/// spelling, takes the target in `lerp`, keeps `self` in `sub`, `add`
/// and `scale`, adds nothing to the magnitude, and is `Default` in
/// `zero`.
#[test]
fn derived_methods_split_animated_from_snapped_fields() {
    let a = Probe {
        x: 1.0,
        v: Vec2::new(2.0, 4.0),
        label: 3,
        mode: 5,
    };
    let b = Probe {
        x: 5.0,
        v: Vec2::new(6.0, 0.0),
        label: 7,
        mode: 9,
    };
    // 1 + (5 − 1)·¼ = 2; (2, 4) + ((6, 0) − (2, 4))·¼ = (3, 3).
    let quarter = Probe {
        x: 2.0,
        v: Vec2::new(3.0, 3.0),
        label: 7,
        mode: 9,
    };
    assert_eq!(Probe::lerp(a, b, 0.25), quarter);
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
    // 1² + 2² + 4² = 21; the snap fields add nothing.
    assert_eq!(a.magnitude_squared(), 21.0);
    assert_eq!(
        Probe::zero(),
        Probe {
            x: 0.0,
            v: Vec2::ZERO,
            label: 0,
            mode: 0,
        },
    );

    // All snap: no arithmetic at all, and a zero magnitude.
    let (p, q) = (AllSnap { a: 1, b: 2 }, AllSnap { a: 3, b: 4 });
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

/// The settle distance, which the spring and the duration snap compare
/// against `1.0`. A struct that names a tolerance divides its magnitude by
/// it: `(3² + 4²) / 0.5² = 100`. One that does not sums its fields', each
/// in its own unit: a red channel of `1/4096` is exactly one `RgbaF32`
/// tolerance, `2^-24 / 2^-24 = 1`, beside `Scaled`'s 100, for 101. At the
/// unit-free `f32` tolerance the same channel would weigh
/// `(2.44e-4 / 1e-4)² ≈ 5.96` — so the colour's own tolerance is the one
/// the sum used. Snap fields weigh nothing.
#[test]
fn derived_settle_distance_measures_each_field_in_its_own_unit() {
    let scaled = Scaled { a: 3.0, b: 4.0 };
    assert_eq!(scaled.settle_distance_squared(), 100.0);
    let mixed = Mixed {
        colour: RgbaF32::new(1.0 / 4096.0, 0.0, 0.0, 0.0),
        scaled,
        label: 7,
    };
    assert_eq!(mixed.settle_distance_squared(), 101.0);
    assert_eq!(AllSnap { a: 1, b: 2 }.settle_distance_squared(), 0.0);
}
