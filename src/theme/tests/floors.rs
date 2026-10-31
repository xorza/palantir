use std::fmt::Write as _;

use crate::primitives::paint::color::RgbaF32;
use crate::theme::Theme;
use crate::theme::color_scheme::ColorScheme;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;
use crate::theme::text_edit::TextEditTheme;

/// WCAG 1.4.3: text against what it sits on
/// (<https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum>).
const TEXT: f32 = 4.5;
/// WCAG 1.4.11: a focus ring or a state mark against what it sits on
/// (<https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast>).
const GRAPHIC: f32 = 3.0;
/// Two surfaces a recipe stacks, so one reads apart from the other: the step Ayu Graphite holds
/// between its rungs.
const RUNG: f32 = 1.10;

/// One pair a recipe paints: `ink` over `ground` must clear `floor`.
struct Pair {
    what: &'static str,
    ink: RgbaF32,
    ground: RgbaF32,
    floor: f32,
}

/// Every pair the stock recipes paint from `p`, with its floor. A pair no recipe paints is not
/// here: muted text sits on no `element_strong`.
fn pairs(p: &Palette) -> Vec<Pair> {
    let pair = |what, ink, ground, floor| Pair {
        what,
        ink,
        ground,
        floor,
    };
    let selection = TextEditTheme::from_parts(p, &Metrics::DEFAULT).selection;
    let wash = p.element_mid.mix(selection.with_alpha(1.0), selection.a);
    vec![
        pair("text on window_bg", p.text, p.window_bg, TEXT),
        pair("text on surround", p.text, p.surround, TEXT),
        pair("text on element", p.text, p.element, TEXT),
        pair("text on element_mid", p.text, p.element_mid, TEXT),
        pair("text on element_strong", p.text, p.element_strong, TEXT),
        pair("text on the selection wash", p.text, wash, TEXT),
        // An inactive tab, a menu bar's title.
        pair("text_muted on surround", p.text_muted, p.surround, TEXT),
        // A menu row's shortcut.
        pair("text_muted on element", p.text_muted, p.element, TEXT),
        // A field's placeholder, a hovered menu row's shortcut.
        pair(
            "text_muted on element_mid",
            p.text_muted,
            p.element_mid,
            TEXT,
        ),
        // A check mark, a radio's dot, an on switch's knob.
        pair("on_accent on accent", p.on_accent, p.accent, TEXT),
        pair(
            "border_focused on window_bg",
            p.border_focused,
            p.window_bg,
            GRAPHIC,
        ),
        pair(
            "border_focused on surround",
            p.border_focused,
            p.surround,
            GRAPHIC,
        ),
        pair(
            "border_focused on element_mid",
            p.border_focused,
            p.element_mid,
            GRAPHIC,
        ),
        pair("accent on element_mid", p.accent, p.element_mid, GRAPHIC),
        pair("window_bg to surround", p.window_bg, p.surround, RUNG),
        pair("surround to element_mid", p.surround, p.element_mid, RUNG),
        pair(
            "element_mid to element_strong",
            p.element_mid,
            p.element_strong,
            RUNG,
        ),
    ]
}

/// Each built-in palette clears every floor; a failure names each pair and its ratio.
#[test]
fn every_built_in_palette_clears_its_contrast_floors() {
    let mut misses = String::new();
    for scheme in [ColorScheme::Dark, ColorScheme::Light] {
        for pair in pairs(Palette::for_scheme(scheme)) {
            let ratio = pair.ink.contrast_ratio(pair.ground);
            if ratio < pair.floor {
                writeln!(
                    misses,
                    "{scheme:?}: {} is {ratio:.3}:1, under {}:1",
                    pair.what, pair.floor
                )
                .unwrap();
            }
        }
    }
    assert!(misses.is_empty(), "{misses}");
}

/// The test reads what it claims: a palette with muted ink equal to its field fill fails on
/// exactly the muted pairs over that fill, and on nothing else.
#[test]
fn a_palette_under_a_floor_fails_on_that_pair() {
    let palette = Palette {
        text_muted: Palette::DARK.element_mid,
        ..Palette::DARK
    };
    let failed: Vec<&str> = pairs(&palette)
        .into_iter()
        .filter(|pair| pair.ink.contrast_ratio(pair.ground) < pair.floor)
        .map(|pair| pair.what)
        .collect();
    assert_eq!(
        failed,
        [
            "text_muted on surround",
            "text_muted on element",
            "text_muted on element_mid"
        ]
    );
}

/// The two built-in palettes are what `for_scheme` hands out, and they differ.
#[test]
fn for_scheme_picks_each_built_in_palette() {
    assert_eq!(*Palette::for_scheme(ColorScheme::Dark), Palette::DARK);
    assert_eq!(*Palette::for_scheme(ColorScheme::Light), Palette::LIGHT);
    assert_ne!(Palette::DARK, Palette::LIGHT);
}

/// `Theme::stock` assembles each scheme's palette, and `Theme::default` is the dark one.
#[test]
fn stock_themes_wear_their_scheme_palette() {
    let light = Theme::stock(ColorScheme::Light);
    assert_eq!(light.window_clear, Palette::LIGHT.window_bg);
    assert_eq!(light.switch.indicator, Palette::LIGHT.on_accent);
    assert_eq!(
        light.tooltip.panel.shadow,
        Palette::LIGHT.float_shadow(&Metrics::DEFAULT)
    );
    assert_eq!(Theme::default(), Theme::stock(ColorScheme::Dark));
    assert_ne!(light, Theme::default());
}
