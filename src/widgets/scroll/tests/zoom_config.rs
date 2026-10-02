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
        // zero minimum
        InvalidConfig {
            range: 0.0..=1.0,
            step: 1.03,
        },
        // negative minimum
        InvalidConfig {
            range: -1.0..=1.0,
            step: 1.03,
        },
        // NaN minimum
        InvalidConfig {
            range: f32::NAN..=1.0,
            step: 1.03,
        },
        // infinite minimum
        InvalidConfig {
            range: f32::INFINITY..=f32::INFINITY,
            step: 1.03,
        },
        // negative infinite minimum
        InvalidConfig {
            range: f32::NEG_INFINITY..=1.0,
            step: 1.03,
        },
        // zero maximum
        InvalidConfig {
            range: 0.1..=0.0,
            step: 1.03,
        },
        // negative maximum
        InvalidConfig {
            range: 0.1..=-1.0,
            step: 1.03,
        },
        // NaN maximum
        InvalidConfig {
            range: 0.1..=f32::NAN,
            step: 1.03,
        },
        // infinite maximum
        InvalidConfig {
            range: 0.1..=f32::INFINITY,
            step: 1.03,
        },
        // negative infinite maximum
        InvalidConfig {
            range: 0.1..=f32::NEG_INFINITY,
            step: 1.03,
        },
        // reversed range
        InvalidConfig {
            range: 2.0..=1.0,
            step: 1.03,
        },
        // zero step
        InvalidConfig {
            range: 0.1..=10.0,
            step: 0.0,
        },
        // negative step
        InvalidConfig {
            range: 0.1..=10.0,
            step: -1.0,
        },
        // NaN step
        InvalidConfig {
            range: 0.1..=10.0,
            step: f32::NAN,
        },
        // positive infinite step
        InvalidConfig {
            range: 0.1..=10.0,
            step: f32::INFINITY,
        },
        // negative infinite step
        InvalidConfig {
            range: 0.1..=10.0,
            step: f32::NEG_INFINITY,
        },
    ];

    for case in cases {
        let step_valid = case.step.is_finite() && case.step > 0.0;
        let expected = if step_valid {
            "zoom range must satisfy 0 < min <= max with finite bounds"
        } else {
            "zoom step must be finite and positive"
        };
        crate::common::panic_probe::assert_panics_with(expected, || {
            ZoomConfig::new(case.range.clone(), case.step)
        });
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
