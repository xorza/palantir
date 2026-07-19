use crate::widgets::theme::Theme;

#[test]
fn set_text_scale_is_absolute_and_total() {
    let mut theme = Theme::default();
    let body = theme.text.font_size_px;
    let tooltip = theme.tooltip.text.font_size_px;
    let disabled = theme
        .button
        .looks
        .disabled
        .text
        .as_ref()
        .expect("button disabled has a text override")
        .font_size_px;

    theme.set_text_scale(2.0);
    assert_eq!(theme.text_scale(), 2.0);
    assert!((theme.text.font_size_px - body * 2.0).abs() < 1e-3);
    assert!((theme.tooltip.text.font_size_px - tooltip * 2.0).abs() < 1e-3);
    assert!(
        (theme
            .button
            .looks
            .disabled
            .text
            .as_ref()
            .unwrap()
            .font_size_px
            - disabled * 2.0)
            .abs()
            < 1e-3
    );

    theme.set_text_scale(1.5);
    assert_eq!(theme.text_scale(), 1.5);
    assert!((theme.text.font_size_px - body * 1.5).abs() < 1e-3);

    theme.set_text_scale(1.0);
    assert!((theme.text.font_size_px - body).abs() < 1e-3);
    assert!((theme.tooltip.text.font_size_px - tooltip).abs() < 1e-3);
    assert!(
        (theme
            .button
            .looks
            .disabled
            .text
            .as_ref()
            .unwrap()
            .font_size_px
            - disabled)
            .abs()
            < 1e-3
    );
}

#[test]
fn set_text_scale_reaches_every_font_size() {
    fn walk(path: &str, before: &toml::Value, after: &toml::Value) {
        match (before, after) {
            (toml::Value::Table(before), toml::Value::Table(after)) => {
                assert_eq!(
                    before.keys().collect::<Vec<_>>(),
                    after.keys().collect::<Vec<_>>(),
                    "key set changed at {path}"
                );
                for (key, value) in before {
                    walk(&format!("{path}.{key}"), value, &after[key]);
                }
            }
            (toml::Value::Array(before), toml::Value::Array(after)) => {
                assert_eq!(before.len(), after.len(), "array len changed at {path}");
                for (index, (before, after)) in before.iter().zip(after).enumerate() {
                    walk(&format!("{path}[{index}]"), before, after);
                }
            }
            (toml::Value::Float(before), toml::Value::Float(after))
                if path.ends_with("font_size_px") || path == "theme.text_scale" =>
            {
                assert!(
                    (after - before * 2.0).abs() < 1e-3,
                    "{path}: {after} is not double {before}"
                );
            }
            _ => assert_eq!(before, after, "non-font value changed at {path}"),
        }
    }

    let mut theme = Theme::default();
    let before = toml::Value::try_from(&theme).expect("serialize");
    theme.set_text_scale(2.0);
    let after = toml::Value::try_from(&theme).expect("serialize");
    walk("theme", &before, &after);
}

#[test]
fn theme_deserialization_rejects_invalid_text_scales() {
    let valid = toml::to_string_pretty(&Theme::default()).expect("serialize default theme");

    for (label, literal) in [
        ("zero", "0.0"),
        ("negative", "-1.0"),
        ("not a number", "nan"),
        ("positive infinity", "inf"),
        ("negative infinity", "-inf"),
    ] {
        let invalid = valid.replacen("text_scale = 1.0", &format!("text_scale = {literal}"), 1);
        let error = toml::from_str::<Theme>(&invalid).expect_err(label);
        assert!(
            error
                .to_string()
                .contains("text scale must be finite and positive"),
            "{label}: unexpected serde error: {error}",
        );
    }
}

#[test]
fn scaled_theme_roundtrip_preserves_the_next_absolute_scale_change() {
    let baseline = Theme::default();
    let body_font_size = baseline.text.font_size_px;
    let tooltip_font_size = baseline.tooltip.text.font_size_px;
    let disabled_font_size = baseline
        .button
        .looks
        .disabled
        .text
        .as_ref()
        .expect("button disabled has a text override")
        .font_size_px;
    let mut scaled = baseline;
    scaled.set_text_scale(2.0);

    let serialized = toml::to_string_pretty(&scaled).expect("serialize scaled theme");
    let mut parsed = toml::from_str::<Theme>(&serialized).expect("parse scaled theme");
    assert_eq!(parsed.text_scale(), 2.0);
    assert_eq!(parsed.text.font_size_px, body_font_size * 2.0);
    assert_eq!(parsed.tooltip.text.font_size_px, tooltip_font_size * 2.0);
    assert_eq!(
        parsed
            .button
            .looks
            .disabled
            .text
            .as_ref()
            .expect("button disabled has a text override")
            .font_size_px,
        disabled_font_size * 2.0,
    );

    parsed.set_text_scale(1.5);
    assert_eq!(parsed.text_scale(), 1.5);
    assert_eq!(parsed.text.font_size_px, body_font_size * 1.5);
    assert_eq!(parsed.tooltip.text.font_size_px, tooltip_font_size * 1.5);
    assert_eq!(
        parsed
            .button
            .looks
            .disabled
            .text
            .as_ref()
            .expect("button disabled has a text override")
            .font_size_px,
        disabled_font_size * 1.5,
    );
}
