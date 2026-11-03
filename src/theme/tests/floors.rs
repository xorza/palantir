use std::fmt::Write as _;

use crate::primitives::paint::backdrop_filter::{BackdropFilter, Lens};
use crate::primitives::paint::color::RgbaF32;
use crate::theme::Theme;
use crate::theme::button::ButtonTheme;
use crate::theme::color_picker::ColorPickerTheme;
use crate::theme::color_scheme::ColorScheme;
use crate::theme::combo_box::ComboBoxTheme;
use crate::theme::dock::DockTheme;
use crate::theme::drag_value::DragValueTheme;
use crate::theme::expander::ExpanderTheme;
use crate::theme::focus_ring::FocusRingTheme;
use crate::theme::look::WidgetLook;
use crate::theme::menu::MenuTheme;
use crate::theme::menu::menu_item::MenuItemTheme;
use crate::theme::menu_bar::MenuBarTheme;
use crate::theme::metrics::{FloatMaterial, Metrics};
use crate::theme::modal::ModalTheme;
use crate::theme::palette::Palette;
use crate::theme::popup::PopupTheme;
use crate::theme::progress_bar::ProgressBarTheme;
use crate::theme::scrollbar::ScrollbarTheme;
use crate::theme::separator::SeparatorTheme;
use crate::theme::slider::SliderTheme;
use crate::theme::spinner::SpinnerTheme;
use crate::theme::splitter::SplitterTheme;
use crate::theme::tabs::TabsTheme;
use crate::theme::text_edit::TextEditTheme;
use crate::theme::text_style::TextStyle;
use crate::theme::toggle::ToggleTheme;
use crate::theme::toggle_kind::ToggleKind;
use crate::theme::tooltip::TooltipTheme;

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
    let accent_button = ButtonTheme::accent(p, &Metrics::DEFAULT).unselected;
    let fill = |look: &WidgetLook| look.chrome.fill.as_solid().expect("a solid fill");
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
        // An accent button's label, hovered and pressed.
        pair(
            "on_accent on the hovered accent button",
            p.on_accent,
            fill(&accent_button.hovered),
            TEXT,
        ),
        pair(
            "on_accent on the pressed accent button",
            p.on_accent,
            fill(&accent_button.active),
            TEXT,
        ),
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

/// `Theme::stock` assembles each scheme's palette, the dark one from `Palette::DARK`.
#[test]
fn stock_themes_wear_their_scheme_palette() {
    let light = Theme::stock(ColorScheme::Light);
    assert_eq!(light.window_bg, Palette::LIGHT.window_bg);
    assert_eq!(light.switch.indicator, Palette::LIGHT.on_accent);
    assert_eq!(
        light.tooltip.panel.shadow,
        Palette::LIGHT.float_shadow(&Metrics::DEFAULT)
    );
    let dark = Theme::stock(ColorScheme::Dark);
    assert_eq!(dark.window_bg, Palette::DARK.window_bg);
    assert_ne!(light, dark);
}

/// Each part's `stock(scheme)` is the part `Theme::stock(scheme)` holds, in either scheme.
#[test]
fn each_parts_stock_is_the_stock_themes_part() {
    for scheme in [ColorScheme::Dark, ColorScheme::Light] {
        let t = Theme::stock(scheme);
        assert_eq!(ButtonTheme::stock(scheme), t.button, "{scheme:?}");
        assert_eq!(ScrollbarTheme::stock(scheme), t.scrollbar, "{scheme:?}");
        assert_eq!(TextEditTheme::stock(scheme), t.text_edit, "{scheme:?}");
        assert_eq!(DragValueTheme::stock(scheme), t.drag_value, "{scheme:?}");
        assert_eq!(MenuTheme::stock(scheme), t.menu, "{scheme:?}");
        assert_eq!(MenuItemTheme::stock(scheme), t.menu.item, "{scheme:?}");
        assert_eq!(MenuBarTheme::stock(scheme), t.menu_bar, "{scheme:?}");
        assert_eq!(ComboBoxTheme::stock(scheme), t.combo_box, "{scheme:?}");
        assert_eq!(ModalTheme::stock(scheme), t.modal, "{scheme:?}");
        assert_eq!(PopupTheme::stock(scheme), t.popup, "{scheme:?}");
        assert_eq!(
            ColorPickerTheme::stock(scheme),
            t.color_picker,
            "{scheme:?}"
        );
        assert_eq!(TooltipTheme::stock(scheme), t.tooltip, "{scheme:?}");
        assert_eq!(
            ProgressBarTheme::stock(scheme),
            t.progress_bar,
            "{scheme:?}"
        );
        assert_eq!(SeparatorTheme::stock(scheme), t.separator, "{scheme:?}");
        assert_eq!(SliderTheme::stock(scheme), t.slider, "{scheme:?}");
        assert_eq!(SpinnerTheme::stock(scheme), t.spinner, "{scheme:?}");
        assert_eq!(SplitterTheme::stock(scheme), t.splitter, "{scheme:?}");
        assert_eq!(TabsTheme::stock(scheme), t.tabs, "{scheme:?}");
        assert_eq!(DockTheme::stock(scheme), t.dock, "{scheme:?}");
        assert_eq!(ExpanderTheme::stock(scheme), t.expander, "{scheme:?}");
        assert_eq!(FocusRingTheme::stock(scheme), t.focus_ring, "{scheme:?}");
        assert_eq!(TextStyle::stock(scheme), t.text, "{scheme:?}");
        for (kind, part) in [
            (ToggleKind::Checkbox, &t.checkbox),
            (ToggleKind::Radio, &t.radio),
            (ToggleKind::Switch, &t.switch),
        ] {
            assert_eq!(
                &ToggleTheme::stock(kind, scheme),
                part,
                "{kind:?} {scheme:?}"
            );
        }
        // The recipes hold each float for the ink the theme paints it in: dressing the stock
        // theme in its own material, as `dress_floats` reads the inks from it, changes nothing.
        let material = Metrics::DEFAULT.float_material.expect("the stock material");
        let mut dressed = t.clone();
        dressed.dress_floats(material);
        assert_eq!(dressed, t, "{scheme:?}");
        // The stock floats wear the frost, none left opaque by an ink it cannot hold.
        for (float, panel) in [
            ("menu", &t.menu.panel),
            ("popup", &t.popup.panel),
            ("picker popup", &t.color_picker.popup.panel),
            ("tooltip", &t.tooltip.panel),
            ("modal", &t.modal.panel),
        ] {
            assert_eq!(panel.backdrop_filter.blur, 20.0, "{float} {scheme:?}");
            assert_eq!(
                panel.fill.as_solid().map(|fill| fill.a),
                Some(184.0 / 255.0),
                "{float} {scheme:?}"
            );
        }
    }
    assert_ne!(
        TextStyle::stock(ColorScheme::Dark).color,
        TextStyle::stock(ColorScheme::Light).color
    );
}

/// `dress_floats` dresses exactly the five floating surfaces: each keeps its fill's color at the
/// tint's alpha, over the material's filter whose bound holds its weakest ink (muted on a menu,
/// the picker and a bare popup, full on a tooltip and the modal), a ceiling under the dark
/// palette's light inks and a floor under the light palette's dark ones; every other slot is as
/// `from_parts` built it. A bound the material names is replaced, and its look is kept: a glass
/// material dresses the floats in glass.
#[test]
fn dress_floats_dresses_each_floating_surface_and_nothing_else() {
    for scheme in [ColorScheme::Dark, ColorScheme::Light] {
        let p = Palette::for_scheme(scheme);
        let stock = Theme::from_parts(p, &Metrics::DEFAULT);
        let mut frosted = stock.clone();
        frosted.dress_floats(FROSTED);
        let panels = |theme: &Theme| {
            [
                theme.menu.panel.clone(),
                theme.color_picker.popup.panel.clone(),
                theme.popup.panel.clone(),
                theme.tooltip.panel.clone(),
                theme.modal.panel.clone(),
            ]
        };
        let inks = [p.text_muted, p.text_muted, p.text_muted, p.text, p.text];
        for ((before, after), ink) in panels(&stock).into_iter().zip(panels(&frosted)).zip(inks) {
            let tint = before
                .fill
                .as_solid()
                .expect("a solid fill")
                .with_alpha(0.6);
            assert_eq!(after.fill.as_solid(), Some(tint), "{scheme:?}");
            let filter = BackdropFilter::frosted(20.0);
            let want = match scheme {
                ColorScheme::Dark => filter.with_max_luminance_for(ink, tint),
                ColorScheme::Light => filter.with_min_luminance_for(ink, tint),
            };
            assert_eq!(after.backdrop_filter, want, "{scheme:?}");
            assert_eq!(after.border, before.border);
            assert_eq!(after.shadow, before.shadow);
        }
        frosted.menu.panel = stock.menu.panel.clone();
        frosted.color_picker.popup = stock.color_picker.popup.clone();
        frosted.popup.panel = stock.popup.panel.clone();
        frosted.tooltip.panel = stock.tooltip.panel.clone();
        frosted.modal.panel = stock.modal.panel.clone();
        assert_eq!(frosted, stock, "{scheme:?}: nothing else moved");
    }

    // The bound reads the theme's ink, not a palette's: a menu whose shortcuts take the full ink
    // gets the full ink's ceiling, which the muted one's is not.
    let mut theme = Theme::stock(ColorScheme::Dark);
    let ink = Palette::DARK.text;
    theme.menu.item.shortcut = ink;
    let tint = theme
        .menu
        .panel
        .fill
        .as_solid()
        .expect("solid")
        .with_alpha(0.6);
    theme.dress_floats(FROSTED);
    let frosted = BackdropFilter::frosted(20.0);
    assert_eq!(
        theme.menu.panel.backdrop_filter,
        frosted.with_max_luminance_for(ink, tint)
    );
    assert_ne!(
        theme.menu.panel.backdrop_filter,
        frosted.with_max_luminance_for(Palette::DARK.text_muted, tint)
    );

    // Any look: glass with a fallback, and a floor it names that the ink's ceiling replaces.
    let glass = BackdropFilter::glass(Lens::WINDOW_GLASS)
        .with_fallback(RgbaF32::hex(0x202020))
        .with_min_luminance(0.5);
    let mut theme = Theme::stock(ColorScheme::Dark);
    theme.dress_floats(FloatMaterial {
        filter: glass,
        tint_alpha: 0.6,
    });
    let tint = theme.tooltip.panel.fill.as_solid().expect("solid");
    let unbound = BackdropFilter {
        min_luminance: None,
        ..glass
    };
    assert_eq!(
        theme.tooltip.panel.backdrop_filter,
        unbound.with_max_luminance_for(Palette::DARK.text, tint)
    );
}

/// The frosted material the tests dress floats in: σ 20 under a 0.6 tint.
const FROSTED: FloatMaterial = FloatMaterial {
    filter: BackdropFilter::frosted(20.0),
    tint_alpha: 0.6,
};

/// A blur that is no length, and a tint the glass could not see through, are refused.
#[test]
fn dress_floats_refuses_a_bad_blur_or_tint() {
    let p = &Palette::DARK;
    for (sigma, alpha, rule) in [
        (-1.0, 0.6, "a length must"),
        (20.0, 1.0, "a fraction under 1"),
    ] {
        let panic = std::panic::catch_unwind(|| {
            Theme::from_parts(p, &Metrics::DEFAULT).dress_floats(FloatMaterial {
                filter: BackdropFilter::frosted(sigma),
                tint_alpha: alpha,
            });
        })
        .expect_err("refused");
        let message = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .unwrap_or_default();
        assert!(message.contains(rule), "{sigma}, {alpha}: {message}");
    }
}

/// The selection wash takes the most of the accent that keeps the text at 4.5:1 over a field, up
/// to 61/255: the dark palette's lime takes 27 (the wash at 27 holds 4.557:1, at 28 4.485:1), the
/// old blue `#44b5ff` all 61 (4.527:1; 62 would read 4.495:1).
#[test]
fn the_selection_wash_takes_what_the_text_allows() {
    let alpha = |accent: RgbaF32| {
        let p = Palette {
            accent,
            ..Palette::DARK
        };
        TextEditTheme::from_parts(&p, &Metrics::DEFAULT).selection
    };
    assert_eq!(
        alpha(RgbaF32::hex(0xd1ff02)),
        RgbaF32::hex(0xd1ff02).with_alpha8(27)
    );
    assert_eq!(
        alpha(RgbaF32::hex(0x44b5ff)),
        RgbaF32::hex(0x44b5ff).with_alpha8(61)
    );
}
