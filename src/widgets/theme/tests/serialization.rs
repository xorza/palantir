use crate::primitives::geometry::corners::Corners;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::shadow::Shadow;
use crate::primitives::paint::stroke::Stroke;
use crate::text::font_family::FontFamily;
use crate::text::font_slant::FontSlant;
use crate::text::font_weight::FontWeight;
use crate::text::glyph_font::GlyphFont;
use crate::widget_core::widget_look::WidgetLook;
use crate::widgets::theme::Theme;
use crate::widgets::theme::text_style::TextStyleOverrides;

use super::pretty;

#[test]
fn default_theme_roundtrips_through_ron() {
    let theme = Theme::default();
    let serialized = pretty(&theme);
    let parsed: Theme = ron::from_str(&serialized).expect("parse");
    let reserialized = pretty(&parsed);
    assert_eq!(serialized, reserialized);
}

#[test]
fn widget_look_serde_roundtrip() {
    let cases = [
        WidgetLook::default(),
        WidgetLook {
            background: Background {
                fill: RgbaF32::hex(0x336699).into(),
                border: Stroke::new(RgbaF32::hex(0xffffff), 1.5),
                corners: Corners::all(6.0),
                shadow: Shadow::NONE,
            },
            text: TextStyleOverrides::NONE.with_font_size(20.0),
        },
        WidgetLook {
            background: Background::NONE,
            text: TextStyleOverrides::NONE.with_color(RgbaF32::hex(0x808080)),
        },
        WidgetLook {
            background: Background::NONE,
            text: TextStyleOverrides {
                family: Some(FontFamily::MONO),
                weight: Some(FontWeight::BOLD),
                slant: Some(FontSlant::Italic),
                ..TextStyleOverrides::NONE
                    .with_font_size(13.0)
                    .with_line_height_factor(1.5)
            },
        },
    ];
    for look in cases {
        let serialized = pretty(&look);
        let parsed: WidgetLook = ron::from_str(&serialized).expect("parse");
        assert_eq!(look, parsed);
        // A file names only the axes a look overrides; an inheriting look writes no `text`.
        assert_eq!(
            serialized.contains("text:"),
            !look.text.is_empty(),
            "{serialized}"
        );
        assert!(!serialized.contains("None"), "{serialized}");
    }
}

/// An override is checked as far as the axes it names reach: a size or leading alone, both together as the face they make. `16 × 0.000001 = 0.000016` is under EPS = 0.0001 though each half is valid alone.
#[test]
fn text_overrides_reject_invalid_metrics_on_load() {
    for (label, ron, ok) in [
        ("empty", "()", true),
        ("colour only", "(color: Some(\"#ff0000\"))", true),
        ("size", "(font_size: Some(13.0))", true),
        ("zero size", "(font_size: Some(0.0))", false),
        ("NaN size", "(font_size: Some(NaN))", false),
        ("sub-epsilon size", "(font_size: Some(0.00005))", false),
        ("leading", "(line_height_factor: Some(0.5))", true),
        (
            "tiny leading alone",
            "(line_height_factor: Some(0.000001))",
            true,
        ),
        ("zero leading", "(line_height_factor: Some(0.0))", false),
        (
            "negative leading",
            "(line_height_factor: Some(-1.0))",
            false,
        ),
        ("infinite leading", "(line_height_factor: Some(inf))", false),
        (
            "sub-epsilon face",
            "(font_size: Some(16.0), line_height_factor: Some(0.000001))",
            false,
        ),
        (
            "valid face",
            "(font_size: Some(16.0), line_height_factor: Some(1.5))",
            true,
        ),
    ] {
        match ron::from_str::<TextStyleOverrides>(ron) {
            Ok(_) => assert!(ok, "{label}: loaded"),
            Err(error) => {
                assert!(!ok, "{label}: rejected: {error}");
                assert!(
                    error.to_string().contains(GlyphFont::METRICS_ERROR),
                    "{label}: unexpected serde error: {error}",
                );
            }
        }
    }
}
