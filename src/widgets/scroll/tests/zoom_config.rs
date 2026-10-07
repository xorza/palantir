use crate::internals::panic_probe;
use crate::primitives::math::domain;
use crate::widgets::scroll::{ZoomConfig, ZoomModifier, ZoomPivot};
use std::ops::RangeInclusive;

#[derive(Debug)]
struct InvalidConfig {
    range: RangeInclusive<f32>,
    step: f32,
}

#[test]
fn zoom_config_rejects_every_invalid_boundary() {
    let cases = [
        InvalidConfig {
            range: 0.0..=1.0,
            step: 1.03,
        },
        InvalidConfig {
            range: -1.0..=1.0,
            step: 1.03,
        },
        InvalidConfig {
            range: f32::NAN..=1.0,
            step: 1.03,
        },
        InvalidConfig {
            range: f32::INFINITY..=f32::INFINITY,
            step: 1.03,
        },
        InvalidConfig {
            range: f32::NEG_INFINITY..=1.0,
            step: 1.03,
        },
        InvalidConfig {
            range: 0.1..=0.0,
            step: 1.03,
        },
        InvalidConfig {
            range: 0.1..=-1.0,
            step: 1.03,
        },
        InvalidConfig {
            range: 0.1..=f32::NAN,
            step: 1.03,
        },
        InvalidConfig {
            range: 0.1..=f32::INFINITY,
            step: 1.03,
        },
        InvalidConfig {
            range: 0.1..=f32::NEG_INFINITY,
            step: 1.03,
        },
        InvalidConfig {
            range: 0.1..=10.0,
            step: 0.0,
        },
        InvalidConfig {
            range: 0.1..=10.0,
            step: -1.0,
        },
        InvalidConfig {
            range: 0.1..=10.0,
            step: f32::NAN,
        },
        InvalidConfig {
            range: 0.1..=10.0,
            step: f32::INFINITY,
        },
        InvalidConfig {
            range: 0.1..=10.0,
            step: f32::NEG_INFINITY,
        },
    ];

    for case in cases {
        panic_probe::assert_panics_with(domain::POSITIVE_RULE, || {
            ZoomConfig::new(case.range.clone(), case.step)
        });
    }

    // A reversed range is coerced into order rather than refused.
    assert_eq!(ZoomConfig::new(2.0..=1.0, 1.03).range, 1.0..=2.0);
}

#[test]
fn zoom_config_accepts_equal_finite_bounds_and_preserves_defaults() {
    let config = ZoomConfig::new(2.0..=2.0, 1.0);
    assert_eq!(config.range, 2.0..=2.0);
    assert_eq!(config.step, 1.0);
    assert_eq!(config.modifier, ZoomModifier::Ctrl);
    assert_eq!(config.pivot, ZoomPivot::Pointer);
}
