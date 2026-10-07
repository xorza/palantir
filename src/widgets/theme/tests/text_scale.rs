use crate::internals::panic_probe;
use ron::Value;

use super::pretty;
use crate::widgets::theme::Theme;
use ron::ser;

/// The size a disabled button shapes its label at; the look names only colour, so it is `theme.text`'s.
fn disabled_size(theme: &Theme) -> f32 {
    theme
        .button
        .looks
        .disabled
        .to_animated(theme.text)
        .text
        .font_size
}

/// The size the tooltip names for its text.
fn tooltip_size(theme: &Theme) -> f32 {
    theme
        .tooltip
        .text
        .font_size
        .expect("the tooltip overrides the size")
}

/// The size the picker's value chips name for themselves.
fn value_size(theme: &Theme) -> f32 {
    theme
        .color_picker
        .value
        .chip
        .looks
        .normal
        .text
        .font_size
        .expect("the picker's value chips override the size")
}

#[test]
fn scale_text_is_relative_and_total() {
    let mut theme = Theme::default();
    let body = theme.text.font_size;
    let tooltip = tooltip_size(&theme);
    let value = value_size(&theme);
    assert_eq!(disabled_size(&theme), body);

    theme.scale_text(2.0);
    assert_eq!(theme.text.font_size, body * 2.0);
    assert_eq!(tooltip_size(&theme), tooltip * 2.0);
    assert_eq!(value_size(&theme), value * 2.0);
    assert_eq!(disabled_size(&theme), body * 2.0);

    // Composes: 2.0 × 0.75 = 1.5, not 0.75.
    theme.scale_text(0.75);
    assert_eq!(theme.text.font_size, body * 1.5);
    assert_eq!(tooltip_size(&theme), tooltip * 1.5);
    assert_eq!(value_size(&theme), value * 1.5);

    // Inverts to baseline: 1.5 × (1 / 1.5) = 1.0.
    theme.scale_text(1.0 / 1.5);
    assert_eq!(theme.text.font_size, body);
    assert_eq!(tooltip_size(&theme), tooltip);
    assert_eq!(value_size(&theme), value);
    assert_eq!(disabled_size(&theme), body);
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
            // A present `Option` is a node of its own, stepped through rather than compared whole.
            (Value::Option(Some(before)), Value::Option(Some(after))) => {
                walk(path, before, after);
            }
            (Value::Seq(before), Value::Seq(after)) => {
                assert_eq!(before.len(), after.len(), "seq len changed at {path}");
                for (index, (before, after)) in before.iter().zip(after).enumerate() {
                    walk(&format!("{path}[{index}]"), before, after);
                }
            }
            (Value::Number(before), Value::Number(after)) if path.ends_with("font_size") => {
                let (before, after) = (before.into_f64(), after.into_f64());
                assert_eq!(
                    after,
                    before * 2.0,
                    "{path}: {after} is not double {before}"
                );
            }
            _ => assert_eq!(before, after, "non-font value changed at {path}"),
        }
    }

    /// The theme as a generic tree. `Value` can't name an enum variant, but both sides flatten alike, so differences still show.
    fn tree(theme: &Theme) -> Value {
        ron::from_str(&ser::to_string(theme).expect("serialize")).expect("reparse")
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
        ("zero font", "font_size: 16.0", "font_size: 0.0"),
        ("negative font", "font_size: 16.0", "font_size: -1.0"),
        ("sub-epsilon font", "font_size: 16.0", "font_size: 0.00005"),
        ("epsilon font", "font_size: 16.0", "font_size: 0.0001"),
        ("NaN font", "font_size: 16.0", "font_size: NaN"),
        ("infinite font", "font_size: 16.0", "font_size: inf"),
        (
            "zero line height",
            "line_height_factor: 1.2",
            "line_height_factor: 0.0",
        ),
        (
            "negative line height",
            "line_height_factor: 1.2",
            "line_height_factor: -1.0",
        ),
        (
            "sub-epsilon line height",
            "line_height_factor: 1.2",
            "line_height_factor: 0.000001",
        ),
        (
            "epsilon line height",
            "line_height_factor: 1.2",
            "line_height_factor: 0.00000625",
        ),
        (
            "NaN line height",
            "line_height_factor: 1.2",
            "line_height_factor: NaN",
        ),
        (
            "infinite line height",
            "line_height_factor: 1.2",
            "line_height_factor: inf",
        ),
        (
            "zero override font",
            "font_size: Some(13.0)",
            "font_size: Some(0.0)",
        ),
        (
            "NaN override font",
            "font_size: Some(13.0)",
            "font_size: Some(NaN)",
        ),
    ];

    for (label, from, to) in cases {
        let invalid = valid.replacen(from, to, 1);
        assert_ne!(invalid, valid, "{label}: `{from}` is not in the theme");
        let error = ron::from_str::<Theme>(&invalid).expect_err(label);
        assert!(
            error.to_string().contains(GlyphFont::METRICS_ERROR),
            "{label}: unexpected serde error: {error}",
        );
    }
}

#[test]
fn scale_text_rejects_invalid_factors_without_partial_mutation() {
    use crate::primitives::math::domain::EPS;
    const FACTOR: &str = "text scale factor must be finite and positive";
    const RESULT: &str = "text scale would make font size or line height invalid";
    // A look's override is checked as the face it folds into. The first overflows alone (f32::MAX / 2 × 4 is inf; ambient 16 × 4 = 64 is fine). The second is valid alone and at 1× but at 1/1000 its leading 16 × 0.001 × 0.001 = 0.000016 px is under EPS = 0.0001, while the ambient 0.016 × 1.2 is above.
    let big_override = |theme: &mut Theme| {
        theme.button.looks.normal.text.font_size = Some(f32::MAX / 2.0);
    };
    let tight_override = |theme: &mut Theme| {
        theme.button.looks.normal.text.line_height_factor = Some(0.001);
    };
    let untouched = |_: &mut Theme| {};
    for (label, setup, factor, expected) in [
        ("zero", &untouched as &dyn Fn(&mut Theme), 0.0, FACTOR),
        ("negative", &untouched, -1.0, FACTOR),
        ("not a number", &untouched, f32::NAN, FACTOR),
        ("infinite", &untouched, f32::INFINITY, FACTOR),
        ("overflow", &untouched, f32::MAX, RESULT),
        ("sub-epsilon result", &untouched, EPS / 32.0, RESULT),
        ("override size overflow", &big_override, 4.0, RESULT),
        ("override leading", &tight_override, 0.001, RESULT),
    ] {
        let mut theme = Theme::default();
        setup(&mut theme);
        let before = pretty(&theme);
        panic_probe::assert_panics_with(expected, || theme.scale_text(factor));
        let after = pretty(&theme);
        assert_eq!(after, before, "{label}: theme was partially mutated");
    }
    // The default theme takes both factors, so the override is what each rejects.
    for factor in [4.0, 0.001] {
        Theme::default().scale_text(factor);
    }
}

/// A scaled theme is just a theme with bigger fonts: a TOML round-trip reproduces it exactly and a further scale composes off the parsed sizes.
#[test]
fn scaled_theme_survives_a_serde_roundtrip() {
    let baseline = Theme::default();
    let body_font_size = baseline.text.font_size;
    let tooltip_font_size = tooltip_size(&baseline);
    let value_font_size = value_size(&baseline);
    let mut scaled = baseline;
    scaled.scale_text(2.0);

    let serialized = pretty(&scaled);
    let mut parsed = ron::from_str::<Theme>(&serialized).expect("parse scaled theme");
    assert_eq!(parsed.text.font_size, body_font_size * 2.0);
    assert_eq!(tooltip_size(&parsed), tooltip_font_size * 2.0);
    assert_eq!(value_size(&parsed), value_font_size * 2.0);

    parsed.scale_text(0.75);
    assert_eq!(parsed.text.font_size, body_font_size * 1.5);
    assert_eq!(tooltip_size(&parsed), tooltip_font_size * 1.5);
    assert_eq!(value_size(&parsed), value_font_size * 1.5);
}
