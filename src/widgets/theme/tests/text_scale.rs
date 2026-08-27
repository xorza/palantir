use ron::Value;

use super::pretty;
use crate::widgets::theme::Theme;

#[test]
fn scale_text_is_relative_and_total() {
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

    theme.scale_text(2.0);
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

    // Composes: 2.0 × 0.75 = 1.5, not 0.75.
    theme.scale_text(0.75);
    assert!((theme.text.font_size_px - body * 1.5).abs() < 1e-3);
    assert!((theme.tooltip.text.font_size_px - tooltip * 1.5).abs() < 1e-3);

    // And inverts back to the baseline: 1.5 × (1 / 1.5) = 1.0.
    theme.scale_text(1.0 / 1.5);
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
fn scale_text_reaches_every_font_size() {
    fn walk(path: &str, before: &Value, after: &Value) {
        match (before, after) {
            (Value::Map(before), Value::Map(after)) => {
                assert_eq!(
                    before.keys().collect::<Vec<_>>(),
                    after.keys().collect::<Vec<_>>(),
                    "key set changed at {path}"
                );
                for (key, value) in before.iter() {
                    let name = match key {
                        Value::String(name) => name.clone(),
                        other => format!("{other:?}"),
                    };
                    walk(&format!("{path}.{name}"), value, &after[key]);
                }
            }
            // A present `Option` is a node of its own, so it is stepped
            // through rather than compared whole.
            (Value::Option(Some(before)), Value::Option(Some(after))) => {
                walk(path, before, after);
            }
            (Value::Seq(before), Value::Seq(after)) => {
                assert_eq!(before.len(), after.len(), "seq len changed at {path}");
                for (index, (before, after)) in before.iter().zip(after).enumerate() {
                    walk(&format!("{path}[{index}]"), before, after);
                }
            }
            (Value::Number(before), Value::Number(after)) if path.ends_with("font_size_px") => {
                let (before, after) = (before.into_f64(), after.into_f64());
                assert!(
                    (after - before * 2.0).abs() < 1e-3,
                    "{path}: {after} is not double {before}"
                );
            }
            _ => assert_eq!(before, after, "non-font value changed at {path}"),
        }
    }

    /// The theme as a generic tree. `Value` cannot name an enum variant, but
    /// both sides are flattened the same way, so what is compared here still
    /// differs exactly where the theme does.
    fn tree(theme: &Theme) -> Value {
        ron::from_str(&ron::ser::to_string(theme).expect("serialize")).expect("reparse")
    }

    let mut theme = Theme::default();
    let before = tree(&theme);
    theme.scale_text(2.0);
    walk("theme", &before, &tree(&theme));
}

#[test]
fn theme_deserialization_rejects_invalid_text_metrics() {
    use crate::text::glyph_font::GlyphFont;

    let valid = pretty(&Theme::default());
    let cases = [
        ("zero font", "font_size_px: 16.0", "font_size_px: 0.0"),
        ("negative font", "font_size_px: 16.0", "font_size_px: -1.0"),
        (
            "sub-epsilon font",
            "font_size_px: 16.0",
            "font_size_px: 0.00005",
        ),
        ("epsilon font", "font_size_px: 16.0", "font_size_px: 0.0001"),
        ("NaN font", "font_size_px: 16.0", "font_size_px: NaN"),
        ("infinite font", "font_size_px: 16.0", "font_size_px: inf"),
        (
            "zero line height",
            "line_height_mult: 1.2",
            "line_height_mult: 0.0",
        ),
        (
            "negative line height",
            "line_height_mult: 1.2",
            "line_height_mult: -1.0",
        ),
        (
            "sub-epsilon line height",
            "line_height_mult: 1.2",
            "line_height_mult: 0.000001",
        ),
        (
            "epsilon line height",
            "line_height_mult: 1.2",
            "line_height_mult: 0.00000625",
        ),
        (
            "NaN line height",
            "line_height_mult: 1.2",
            "line_height_mult: NaN",
        ),
        (
            "infinite line height",
            "line_height_mult: 1.2",
            "line_height_mult: inf",
        ),
    ];

    for (label, from, to) in cases {
        let invalid = valid.replacen(from, to, 1);
        let error = ron::from_str::<Theme>(&invalid).expect_err(label);
        assert!(
            error.to_string().contains(GlyphFont::METRICS_ERROR),
            "{label}: unexpected serde error: {error}",
        );
    }
}

#[test]
fn scale_text_rejects_invalid_factors_without_partial_mutation() {
    use crate::primitives::approx::EPS;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    for (label, factor) in [
        ("zero", 0.0),
        ("negative", -1.0),
        ("not a number", f32::NAN),
        ("infinite", f32::INFINITY),
        ("overflow", f32::MAX),
        ("sub-epsilon result", EPS / 32.0),
    ] {
        let mut theme = Theme::default();
        let before = pretty(&theme);
        let panic = catch_unwind(AssertUnwindSafe(|| theme.scale_text(factor)));
        assert!(panic.is_err(), "{label}: invalid factor was accepted");
        let after = pretty(&theme);
        assert_eq!(after, before, "{label}: theme was partially mutated");
    }
}

/// A scaled theme is just a theme with bigger fonts: nothing beside the
/// sizes records that a scale was applied, so a round-trip through TOML
/// reproduces it exactly and a further scale composes off the parsed
/// sizes.
#[test]
fn scaled_theme_survives_a_serde_roundtrip() {
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
    scaled.scale_text(2.0);

    let serialized = pretty(&scaled);
    let mut parsed = ron::from_str::<Theme>(&serialized).expect("parse scaled theme");
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

    parsed.scale_text(0.75);
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
