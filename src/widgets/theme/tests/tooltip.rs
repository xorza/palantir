use crate::widgets::theme::tooltip::TooltipTheme;
use std::time::Duration;

#[test]
fn tooltip_theme_defaults() {
    let theme = TooltipTheme::default();
    assert_eq!(theme.delay, Duration::from_millis(500));
    assert_eq!(theme.warmup, Duration::from_secs(1));
    assert!((theme.max_size.w - 280.0).abs() < 1e-6);
    assert!(theme.max_size.h.is_infinite());
    assert!((theme.gap - 6.0).abs() < 1e-6);
}

#[test]
fn tooltip_timing_serde_stays_scalar_and_rejects_invalid_seconds() {
    let serialized = toml::to_string_pretty(&TooltipTheme::default()).expect("serialize");
    assert!(serialized.contains("delay = 0.5"));
    assert!(serialized.contains("warmup = 1.0"));

    for (field, valid, invalid) in [
        ("delay", "0.5", "-0.1"),
        ("delay", "0.5", "nan"),
        ("delay", "0.5", "inf"),
        ("delay", "0.5", "3.4028235e38"),
        ("warmup", "1.0", "-0.1"),
        ("warmup", "1.0", "nan"),
        ("warmup", "1.0", "inf"),
    ] {
        let input = serialized.replace(
            &format!("{field} = {valid}"),
            &format!("{field} = {invalid}"),
        );
        let error = toml::from_str::<TooltipTheme>(&input).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("tooltip timing must be finite, non-negative, and representable"),
            "{field}={invalid}: {error}",
        );
    }

    let zeroed = serialized
        .replace("delay = 0.5", "delay = 0.0")
        .replace("warmup = 1.0", "warmup = 0.0");
    let parsed = toml::from_str::<TooltipTheme>(&zeroed).expect("zero timing");
    assert_eq!(parsed.delay, Duration::ZERO);
    assert_eq!(parsed.warmup, Duration::ZERO);
}
