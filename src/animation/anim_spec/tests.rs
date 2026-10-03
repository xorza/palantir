use crate::animation::anim_spec::AnimSpec;
use crate::animation::duration::DURATION_ERROR;
use crate::animation::easing::Easing;
use crate::animation::spring::SPRING_ERROR;
use crate::internals::panic_probe;
use crate::primitives::math::domain::EPS;
use ron::ser;

#[test]
fn anim_spec_construction_validates_and_canonicalizes() {
    let instant_zero = AnimSpec::duration(0.0, Easing::Linear);
    let instant_negative_zero = AnimSpec::duration(-0.0, Easing::Linear);
    let instant_sub_eps = AnimSpec::duration(EPS * 0.5, Easing::Linear);
    assert!(instant_zero.is_instant());
    assert!(instant_negative_zero.is_instant());
    assert!(instant_sub_eps.is_instant());
    assert!(!AnimSpec::duration(EPS, Easing::Linear).is_instant());
    assert!(!AnimSpec::duration(60.0, Easing::Linear).is_instant());
    assert!(!AnimSpec::FAST.is_instant());
    assert!(!AnimSpec::SPRING.is_instant());

    for secs in [
        -1.0,
        60.0 + f32::EPSILON * 64.0,
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
    ] {
        panic_probe::assert_panics_with(DURATION_ERROR, || {
            AnimSpec::duration(secs, Easing::Linear)
        });
    }

    for (stiffness, damping) in [
        (0.0, 1.0),
        (1.0, 0.0),
        (-1.0, 1.0),
        (1.0, -1.0),
        (f32::NAN, 1.0),
        (1.0, f32::INFINITY),
        (1.0, 1.0),
        (1.0, 100.0),
        // Underdamped past 30 Hz. (60π)² ≈ 35 530.6, so at damping 2
        // (`h² = 1`) stiffness 35 600 swings at √35 599 ≈ 188.7 rad/s.
        (35_600.0, 2.0),
        // ζ = 0.05: ψ = √(1e6 − 2 500) ≈ 999 rad/s, a 159 Hz buzz.
        (1_000_000.0, 100.0),
        (f32::MAX, 2.0),
    ] {
        panic_probe::assert_panics_with(SPRING_ERROR, || AnimSpec::spring(stiffness, damping));
    }

    assert!(!AnimSpec::spring(1.0, 2.0).is_instant());
    // The swing bound's other side, √35 499 ≈ 188.4 rad/s, under 60π.
    assert!(!AnimSpec::spring(35_500.0, 2.0).is_instant());
    // No stiffness is too stiff when the spring does not swing: critically
    // damped at `h² = k`, overdamped past it.
    assert!(!AnimSpec::spring(1_000_000.0, 2_000.0).is_instant());
    assert!(!AnimSpec::spring(f32::MAX, 4.0e19).is_instant());
}

#[test]
fn anim_spec_serde_validates_and_roundtrips() {
    #[derive(::serde::Serialize, ::serde::Deserialize, PartialEq, Debug)]
    struct Holder {
        spec: AnimSpec,
    }
    let cases = [
        AnimSpec::FAST,
        AnimSpec::MEDIUM,
        AnimSpec::SPRING,
        AnimSpec::duration(0.1, Easing::Linear),
        AnimSpec::duration(0.2, Easing::InOutCubic),
        AnimSpec::duration(0.3, Easing::OutQuart),
        AnimSpec::duration(0.4, Easing::OutBack),
        AnimSpec::spring(100.0, 15.0),
        AnimSpec::spring(1_000_000.0, 2_000.0),
        AnimSpec::spring(f32::MAX, 4.0e19),
    ];
    for spec in cases {
        let h = Holder { spec };
        let s = ser::to_string(&h).expect("serialize");
        let back: Holder = ron::from_str(&s).expect("parse");
        assert_eq!(back, h, "roundtrip mismatch for {spec:?}\nRON:\n{s}");
    }

    let canonical: Holder =
        ron::from_str(r#"(spec: (kind: "duration", secs: 0.00005, ease: "linear"))"#)
            .expect("sub-epsilon duration is a valid instant");
    assert!(canonical.spec.is_instant());
    assert!(
        ser::to_string(&canonical)
            .expect("serialize canonical duration")
            .contains("secs:0.0"),
    );

    let invalid = [
        (
            "negative duration",
            r#"(spec: (kind: "duration", secs: -1.0, ease: "linear"))"#,
            DURATION_ERROR,
        ),
        (
            "non-finite duration",
            r#"(spec: (kind: "duration", secs: NaN, ease: "linear"))"#,
            DURATION_ERROR,
        ),
        (
            "non-positive spring",
            r#"(spec: (kind: "spring", stiffness: 170.0, damping: 0.0))"#,
            SPRING_ERROR,
        ),
        (
            "slow spring",
            r#"(spec: (kind: "spring", stiffness: 1.0, damping: 100.0))"#,
            SPRING_ERROR,
        ),
        (
            "swing past 30 Hz",
            r#"(spec: (kind: "spring", stiffness: 1000000.0, damping: 100.0))"#,
            SPRING_ERROR,
        ),
    ];
    for (label, input, expected) in invalid {
        let error = ron::from_str::<Holder>(input).expect_err(label);
        assert!(
            error.to_string().contains(expected),
            "{label}: unexpected serde error: {error}",
        );
    }
}
