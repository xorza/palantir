use crate::animation::animation_spec::AnimationSpec;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::math::domain;
use crate::primitives::paint::border::Border;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::material::Material;
use crate::primitives::paint::material::lens::Lens;
use crate::primitives::paint::material::reduced_backdrop::ReducedBackdrop;
use crate::primitives::paint::paint::Paint;
use crate::primitives::paint::shadow::Shadow;
use crate::text::font_family::FontFamily;
use crate::text::font_slant::FontSlant;
use crate::text::font_weight::FontWeight;
use crate::theme::Theme;
use crate::theme::color_scheme::ColorScheme;
use crate::theme::look::WidgetLook;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;
use crate::theme::text_style_overrides::TextStyleOverrides;

use super::pretty;

/// The default theme saves and loads back to the same value, and every color it holds is on the
/// 8-bit grid, so its file is all hex.
#[test]
fn default_theme_roundtrips_through_ron() {
    let serialized = pretty(&Theme::stock(ColorScheme::Dark));
    let loaded: Theme = ron::from_str(&serialized).expect("parse");
    assert_eq!(loaded, Theme::stock(ColorScheme::Dark));
    assert_eq!(pretty(&loaded), serialized);
    assert!(!serialized.contains("color("), "an off-grid default color");
    // A material costs a capture and a blur each frame what lies under it repaints, so only what
    // floats over content wears one: the menu, popup, picker popup, tooltip and modal panels, and
    // no slot at all without a float material. A file omits a chrome's absent material.
    assert_eq!(serialized.matches("material:").count(), 5);
    let opaque = Metrics {
        float_material: None,
        ..Metrics::DEFAULT
    };
    let opaque = pretty(&Theme::from_parts(&Palette::DARK, &opaque));
    assert_eq!(opaque.matches("material:").count(), 0);

    // A theme's transition is its own value in the file, a duration or a spring.
    for transition in [AnimationSpec::SNAP, AnimationSpec::spring(300.0, 10.0)] {
        let theme = Theme {
            transition,
            ..Theme::stock(ColorScheme::Dark)
        };
        let file = pretty(&theme);
        assert_ne!(file, serialized);
        let loaded: Theme = ron::from_str(&file).expect("parse");
        assert_eq!(loaded.transition, transition);
    }
}

#[test]
fn widget_look_serde_roundtrip() {
    let cases =
        [
            WidgetLook::default(),
            WidgetLook {
                chrome: Chrome {
                    corners: Corners::all(6.0),
                    paint: Paint {
                        fill: RgbaF32::hex(0x336699).into(),
                        border: Border::new(RgbaF32::hex(0xffffff), 1.5),
                        shadow: Shadow::NONE,
                        material: None,
                    },
                },
                text: TextStyleOverrides::NONE.with_font_size(20.0),
            },
            WidgetLook {
                chrome: Chrome::rounded(RgbaF32::hex(0xffffff).with_alpha(0.2), Corners::all(8.0))
                    .with_material(Material::frosted(12.0).with_max_luminance(0.05).with_lens(
                        Lens {
                            dispersion: 0.0,
                            ..Lens::WINDOW_GLASS
                        },
                    )),
                text: TextStyleOverrides::NONE,
            },
            WidgetLook {
                chrome: Chrome::NONE,
                text: TextStyleOverrides::NONE.with_color(RgbaF32::hex(0x808080)),
            },
            WidgetLook {
                chrome: Chrome::NONE,
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

/// A material in a file names its effect and only what differs from its defaults, and each field
/// is checked by its kind: a blur a length, saturation, noise and a luminance ceiling factors, a
/// lens's index at least 1 and its dispersion and highlight fractions. An effect is a name a file
/// cannot leave out, which the theme's set checks against the loaded effects.
#[test]
fn materials_load_by_their_kinds() {
    let blur = |rest: &str| format!("(effect: \"palantir.blur\"{rest})");
    for (label, ron, want) in [
        ("blur alone", blur(", blur: 8.0"), Some(Material::blur(8.0))),
        (
            "gray",
            blur(", input: (saturation: 0.0)"),
            Some(Material::blur(0.0).with_saturation(0.0)),
        ),
        (
            "default lens",
            "(effect: \"palantir.glass\", input: (lens: ()))".to_owned(),
            Some(Material::glass(Lens::WINDOW_GLASS)),
        ),
        (
            "keeps its dim",
            blur(", reduced_transparency: Nothing"),
            Some(Material::blur(0.0).with_reduced_transparency(ReducedBackdrop::Nothing)),
        ),
        ("no effect", "(blur: 8.0)".to_owned(), None),
        ("negative blur", blur(", blur: -1.0"), None),
        ("NaN saturation", blur(", input: (saturation: NaN)"), None),
        ("negative noise", blur(", input: (noise: -0.1)"), None),
        (
            "ceiling",
            blur(", max_luminance: Some(0.05)"),
            Some(Material::blur(0.0).with_max_luminance(0.05)),
        ),
        (
            "no ceiling",
            blur(", max_luminance: None"),
            Some(Material::blur(0.0)),
        ),
        (
            "negative ceiling",
            blur(", max_luminance: Some(-0.1)"),
            None,
        ),
        ("NaN ceiling", blur(", max_luminance: Some(NaN)"), None),
        ("infinite ceiling", blur(", max_luminance: Some(inf)"), None),
        (
            "index below 1",
            blur(", input: (lens: (refractive_index: 0.9))"),
            None,
        ),
        (
            "dispersion past 1",
            blur(", input: (lens: (dispersion: 1.5))"),
            None,
        ),
    ] {
        let got = ron::from_str::<Material>(&ron).ok();
        assert_eq!(got, want, "{label}");
    }
}

/// An override is checked as far as the axes it names reach: a size or leading alone, both together as the face they make. `16 × 0.000001 = 0.000016` is under EPS = 0.0001 though each half is valid alone.
#[test]
fn text_overrides_reject_invalid_metrics_on_load() {
    for (label, ron, ok) in [
        ("empty", "()", true),
        ("color only", "(color: Some(\"#ff0000\"))", true),
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
                    error.to_string().contains(domain::FONT_LENGTH_RULE),
                    "{label}: unexpected serde error: {error}",
                );
            }
        }
    }
}
