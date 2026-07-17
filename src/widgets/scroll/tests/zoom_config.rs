use crate::widgets::scroll::{ZoomConfig, ZoomModifier, ZoomPivot};
use std::ops::RangeInclusive;

#[derive(Debug)]
struct InvalidConfig {
    label: &'static str,
    range: RangeInclusive<f32>,
    step: f32,
}

#[test]
fn zoom_config_rejects_every_invalid_boundary() {
    let cases = [
        InvalidConfig {
            label: "zero minimum",
            range: 0.0..=1.0,
            step: 1.03,
        },
        InvalidConfig {
            label: "negative minimum",
            range: -1.0..=1.0,
            step: 1.03,
        },
        InvalidConfig {
            label: "NaN minimum",
            range: f32::NAN..=1.0,
            step: 1.03,
        },
        InvalidConfig {
            label: "infinite minimum",
            range: f32::INFINITY..=f32::INFINITY,
            step: 1.03,
        },
        InvalidConfig {
            label: "negative infinite minimum",
            range: f32::NEG_INFINITY..=1.0,
            step: 1.03,
        },
        InvalidConfig {
            label: "zero maximum",
            range: 0.1..=0.0,
            step: 1.03,
        },
        InvalidConfig {
            label: "negative maximum",
            range: 0.1..=-1.0,
            step: 1.03,
        },
        InvalidConfig {
            label: "NaN maximum",
            range: 0.1..=f32::NAN,
            step: 1.03,
        },
        InvalidConfig {
            label: "infinite maximum",
            range: 0.1..=f32::INFINITY,
            step: 1.03,
        },
        InvalidConfig {
            label: "negative infinite maximum",
            range: 0.1..=f32::NEG_INFINITY,
            step: 1.03,
        },
        InvalidConfig {
            label: "reversed range",
            range: 2.0..=1.0,
            step: 1.03,
        },
        InvalidConfig {
            label: "zero step",
            range: 0.1..=10.0,
            step: 0.0,
        },
        InvalidConfig {
            label: "negative step",
            range: 0.1..=10.0,
            step: -1.0,
        },
        InvalidConfig {
            label: "NaN step",
            range: 0.1..=10.0,
            step: f32::NAN,
        },
        InvalidConfig {
            label: "positive infinite step",
            range: 0.1..=10.0,
            step: f32::INFINITY,
        },
        InvalidConfig {
            label: "negative infinite step",
            range: 0.1..=10.0,
            step: f32::NEG_INFINITY,
        },
    ];

    for case in cases {
        assert!(
            std::panic::catch_unwind(|| ZoomConfig::new(case.range, case.step)).is_err(),
            "{}",
            case.label,
        );
    }
}

#[test]
fn zoom_config_accepts_equal_finite_bounds_and_preserves_defaults() {
    let config = ZoomConfig::new(2.0..=2.0, 1.0);
    assert_eq!(config.range, 2.0..=2.0);
    assert_eq!(config.step, 1.0);
    assert_eq!(config.modifier, ZoomModifier::Ctrl);
    assert_eq!(config.pivot, ZoomPivot::Pointer);
}
