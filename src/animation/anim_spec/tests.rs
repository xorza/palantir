use crate::animation::anim_spec::AnimSpec;
use crate::animation::easing::Easing;
use crate::primitives::approx::EPS;

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
        assert!(
            std::panic::catch_unwind(|| AnimSpec::duration(secs, Easing::Linear)).is_err(),
            "duration constructor accepted {secs:?}",
        );
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
        (f32::MAX, 2.0),
    ] {
        assert!(
            std::panic::catch_unwind(|| AnimSpec::spring(stiffness, damping)).is_err(),
            "spring constructor accepted ({stiffness:?}, {damping:?})",
        );
    }

    assert!(!AnimSpec::spring(1.0, 2.0).is_instant());
    assert!(!AnimSpec::spring(1_000_000.0, 100.0).is_instant());
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
        AnimSpec::spring(1_000_000.0, 100.0),
    ];
    for spec in cases {
        let h = Holder { spec };
        let s = ron::ser::to_string(&h).expect("serialize");
        let back: Holder = ron::from_str(&s).expect("parse");
        assert_eq!(back, h, "roundtrip mismatch for {spec:?}\nRON:\n{s}");
    }

    let canonical: Holder =
        ron::from_str(r#"(spec: (kind: "duration", secs: 0.00005, ease: "linear"))"#)
            .expect("sub-epsilon duration is a valid instant");
    assert!(canonical.spec.is_instant());
    assert!(
        ron::ser::to_string(&canonical)
            .expect("serialize canonical duration")
            .contains("secs:0.0"),
    );

    let invalid = [
        (
            "negative duration",
            r#"(spec: (kind: "duration", secs: -1.0, ease: "linear"))"#,
            "animation duration must be finite and in 0.0..=60.0 seconds",
        ),
        (
            "non-finite duration",
            r#"(spec: (kind: "duration", secs: NaN, ease: "linear"))"#,
            "animation duration must be finite and in 0.0..=60.0 seconds",
        ),
        (
            "non-positive spring",
            r#"(spec: (kind: "spring", stiffness: 170.0, damping: 0.0))"#,
            "spring parameters must be positive, finite, convergent, and within the integration limit",
        ),
        (
            "slow spring",
            r#"(spec: (kind: "spring", stiffness: 1.0, damping: 100.0))"#,
            "spring parameters must be positive, finite, convergent, and within the integration limit",
        ),
        (
            "expensive spring",
            r#"(spec: (kind: "spring", stiffness: 3.4028235e38, damping: 2.0))"#,
            "spring parameters must be positive, finite, convergent, and within the integration limit",
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
