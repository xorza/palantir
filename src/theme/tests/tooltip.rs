use super::pretty;
use crate::theme::spinner::SpinnerTheme;
use crate::theme::tooltip::TooltipTheme;
use std::time::Duration;

#[test]
fn tooltip_theme_defaults() {
    let theme = TooltipTheme::default();
    assert_eq!(theme.delay, Duration::from_millis(500));
    assert_eq!(theme.warmup, Duration::from_secs(1));
    assert_eq!(theme.max_size.w, 280.0);
    assert!(theme.max_size.h.is_infinite());
    assert_eq!(theme.gap, 6.0);
}

#[test]
fn tooltip_timing_serde_stays_scalar_and_rejects_invalid_seconds() {
    let serialized = pretty(&TooltipTheme::default());
    assert!(serialized.contains("delay: 0.5"));
    assert!(serialized.contains("warmup: 1.0"));

    for (field, valid, invalid) in [
        ("delay", "0.5", "-0.1"),
        ("delay", "0.5", "NaN"),
        ("delay", "0.5", "inf"),
        ("delay", "0.5", "3.4028235e38"),
        ("warmup", "1.0", "-0.1"),
        ("warmup", "1.0", "NaN"),
        ("warmup", "1.0", "inf"),
    ] {
        let input =
            serialized.replace(&format!("{field}: {valid}"), &format!("{field}: {invalid}"));
        let error = ron::from_str::<TooltipTheme>(&input).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("a duration must be finite, not negative, and representable"),
            "{field}={invalid}: {error}",
        );
    }

    let zeroed = serialized
        .replace("delay: 0.5", "delay: 0.0")
        .replace("warmup: 1.0", "warmup: 0.0");
    let parsed = ron::from_str::<TooltipTheme>(&zeroed).expect("zero timing");
    assert_eq!(parsed.delay, Duration::ZERO);
    assert_eq!(parsed.warmup, Duration::ZERO);
}

/// Seconds travel as `f64`, so 300 ms reads back to the nanosecond. A spinner's period refuses
/// zero, also the zero that `1e-20` s rounds to, and names the value the file gave.
#[test]
fn durations_read_back_exactly_and_a_period_refuses_zero() {
    let tooltip = TooltipTheme {
        delay: Duration::from_millis(300),
        ..TooltipTheme::default()
    };
    let parsed = ron::from_str::<TooltipTheme>(&pretty(&tooltip)).expect("parse");
    assert_eq!(parsed.delay, Duration::from_millis(300));

    let spinner = pretty(&SpinnerTheme::default());
    assert!(spinner.contains("period: 1.4"), "{spinner}");
    for (bad, read) in [
        ("0.0", "0"),
        ("1e-20", "0.00000000000000000001"),
        ("-1.0", "-1"),
    ] {
        let input = spinner.replace("period: 1.4", &format!("period: {bad}"));
        let error = ron::from_str::<SpinnerTheme>(&input)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(&format!(
                "a period must be a duration above zero, got {read}"
            )),
            "{bad}: {error}",
        );
    }
}
