use crate::input::interaction::button_phase::ButtonPhase;
use crate::input::interaction::button_state::ButtonState;
use crate::input::interaction::response_state::ResponseState;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::text::font_family::FontFamily;
use crate::text::font_slant::FontSlant;
use crate::text::font_weight::FontWeight;
use crate::theme::button::ButtonTheme;
use crate::theme::check_state::CheckState;
use crate::theme::color_scheme::ColorScheme;
use crate::theme::look::WidgetLook;
use crate::theme::look::animated_look::AnimatedLook;
use crate::theme::look::engagement::Engagement;
use crate::theme::look::theme_slot::ThemeSlot;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;
use crate::theme::selection::Selection;
use crate::theme::tab_strip_focus::TabStripFocus;
use crate::theme::tabs::TabsTheme;
use crate::theme::text_edit::TextEditTheme;
use crate::theme::text_style::TextStyle;
use crate::theme::text_style_overrides::TextStyleOverrides;
use crate::theme::toggle::ToggleTheme;
use crate::theme::toggle_kind::ToggleKind;
use std::ptr;

/// A response hovered, pressed and disabled as the flags say.
fn response(pointer_over: bool, pressed: bool, disabled: bool) -> ResponseState {
    ResponseState {
        pointer_over,
        left: ButtonState {
            phase: if pressed {
                ButtonPhase::Held
            } else {
                ButtonPhase::Idle
            },
            ..Default::default()
        },
        disabled,
        ..ResponseState::default()
    }
}

/// A button picks its resting or selected pack, then the pressed precedence inside it. The
/// recipe's selected pack wears the pressed look in every live state, so it differs from the
/// resting pack at rest and under the pointer.
#[test]
fn button_theme_pick_precedence() {
    let theme = ButtonTheme::stock(ColorScheme::Dark);
    for (selection, pack) in [
        (Selection::Unselected, &theme.unselected),
        (Selection::Selected, &theme.selected),
    ] {
        let cases: &[(ResponseState, &WidgetLook, &str)] = &[
            (response(false, false, false), &pack.normal, "normal"),
            (response(true, false, false), &pack.hovered, "hovered"),
            (
                response(true, true, false),
                &pack.active,
                "pressed > hovered",
            ),
            (
                response(false, false, true),
                &pack.disabled,
                "disabled (idle)",
            ),
            (
                response(true, true, true),
                &pack.disabled,
                "disabled wins all",
            ),
        ];
        for (state, expected, label) in cases {
            assert!(
                ptr::eq(theme.look(state, selection), *expected),
                "{selection:?} {label}: look should return the matching slot",
            );
        }
    }
    assert_eq!(theme.selected.normal, theme.unselected.active);
    assert_eq!(theme.selected.hovered, theme.unselected.active);
    assert_ne!(theme.selected.normal, theme.unselected.normal);
    assert_ne!(theme.selected.hovered, theme.unselected.hovered);
}

#[test]
fn text_edit_theme_pick_precedence() {
    let theme = TextEditTheme::stock(ColorScheme::Dark);
    let state = |focused, pointer_over, disabled| ResponseState {
        pointer_over,
        disabled,
        focused,
        ..ResponseState::default()
    };
    let cases: &[(ResponseState, &WidgetLook, &str)] = &[
        (state(false, false, false), &theme.looks.normal, "normal"),
        (state(false, true, false), &theme.looks.hovered, "hovered"),
        (state(true, false, false), &theme.looks.active, "focused"),
        (
            state(true, true, false),
            &theme.looks.active,
            "focused wins hover",
        ),
        (
            state(false, false, true),
            &theme.looks.disabled,
            "disabled (unfocused)",
        ),
        (
            state(true, true, true),
            &theme.looks.disabled,
            "disabled wins focus",
        ),
    ];
    for (state, expected, label) in cases {
        assert!(
            ptr::eq(theme.look(state, ()), *expected),
            "{label}: look should return the matching slot",
        );
    }

    // The engagement names the active state: a hovered press engages a button's pack, not a field's.
    let pressed = response(true, true, false);
    assert!(ptr::eq(
        theme.looks.pick(&pressed, Engagement::Focused),
        &raw const theme.looks.hovered,
    ));
    assert!(ptr::eq(
        theme.looks.pick(&pressed, Engagement::Pressed),
        &raw const theme.looks.active,
    ));
}

/// A flat field drops its padding, margin and every state's chrome, and keeps its caret,
/// selection and every state's text.
#[test]
fn a_flat_text_edit_keeps_only_its_text_and_caret() {
    let field = TextEditTheme::stock(ColorScheme::Dark);
    let flat = field.flat();
    assert_eq!(flat.defaults.padding, Spacing::ZERO);
    assert_eq!(flat.defaults.margin, Spacing::ZERO);
    assert_eq!(flat.defaults.transition, field.defaults.transition);
    let (mut flat_looks, mut looks) = (flat.looks.clone(), field.looks.clone());
    for (flat_look, look) in flat_looks.each_mut().into_iter().zip(looks.each_mut()) {
        assert_eq!(flat_look.chrome, Chrome::NONE);
        assert_eq!(flat_look.text, look.text);
    }
    assert_eq!(
        (
            flat.caret,
            flat.caret_width,
            flat.selection,
            flat.placeholder
        ),
        (
            field.caret,
            field.caret_width,
            field.selection,
            field.placeholder
        )
    );
}

/// [`ToggleTheme`]'s pick needs an input the response can't supply: `Pick = CheckState` chooses the look pack, then the four-state precedence runs inside it. Asserts the pack switching on the check state, the precedence within each, and that the packs never resolve to the same slot (so `Pick` is load-bearing).
#[test]
fn toggle_theme_pick_selects_pack_then_state() {
    let theme = ToggleTheme::stock(ToggleKind::Checkbox, ColorScheme::Dark);
    let cases: &[(ResponseState, CheckState, &WidgetLook, &str)] = &[
        (
            response(false, false, false),
            CheckState::Unchecked,
            &theme.unchecked.normal,
            "unchecked normal",
        ),
        (
            response(true, false, false),
            CheckState::Unchecked,
            &theme.unchecked.hovered,
            "unchecked hovered",
        ),
        (
            response(false, false, false),
            CheckState::Checked,
            &theme.checked.normal,
            "checked normal",
        ),
        (
            response(true, false, false),
            CheckState::Checked,
            &theme.checked.hovered,
            "checked hovered",
        ),
        (
            response(true, true, false),
            CheckState::Checked,
            &theme.checked.active,
            "checked: pressed > hovered",
        ),
        (
            response(true, true, true),
            CheckState::Checked,
            &theme.checked.disabled,
            "checked: disabled wins all",
        ),
    ];
    for (state, checked, expected, label) in cases {
        assert!(
            ptr::eq(theme.look(state, *checked), *expected),
            "{label}: look should return the matching slot",
        );
    }

    // The check state alone decides: one state, two packs.
    let idle = response(false, false, false);
    assert!(
        !ptr::eq(
            theme.look(&idle, CheckState::Unchecked),
            theme.look(&idle, CheckState::Checked)
        ),
        "checked and unchecked must not resolve to the same look",
    );
    assert_eq!(CheckState::from(true), CheckState::Checked);
    assert_eq!(CheckState::from(false), CheckState::Unchecked);
}

/// [`TabsTheme`] picks the selected chip's pack, then the pressed precedence inside it; the
/// strip's focus picks the cap color.
#[test]
fn tabs_theme_pick_selects_pack_then_state_and_cap() {
    let theme = TabsTheme::stock(ColorScheme::Dark);
    let idle = ResponseState::default();
    let pressed = response(true, true, false);
    let cases: &[(&ResponseState, Selection, &WidgetLook, &str)] = &[
        (
            &idle,
            Selection::Selected,
            &theme.selected.normal,
            "selected",
        ),
        (
            &idle,
            Selection::Unselected,
            &theme.unselected.normal,
            "unselected",
        ),
        (
            &pressed,
            Selection::Unselected,
            &theme.unselected.active,
            "unselected: pressed",
        ),
    ];
    for (state, selection, expected, label) in cases {
        assert!(
            ptr::eq(theme.look(state, *selection), *expected),
            "{label}: look should return the matching slot",
        );
    }
    // The strip rides the `surround` band, and an unselected chip on it is a ghost: no fill at
    // rest, `element_mid` under the pointer.
    assert_eq!(theme.strip, Chrome::fill(Palette::DARK.surround));
    assert_eq!(theme.unselected.normal.chrome.fill, Chrome::NONE.fill);
    assert_eq!(
        theme.unselected.hovered.chrome.fill,
        Palette::DARK.element_mid.into()
    );
    assert_eq!(Selection::from(true), Selection::Selected);
    assert_eq!(Selection::from(false), Selection::Unselected);
    assert_eq!(TabStripFocus::from(true), TabStripFocus::Focused);
    assert_eq!(TabStripFocus::from(false), TabStripFocus::Unfocused);

    assert_eq!(theme.cap(TabStripFocus::Focused), Palette::DARK.accent);
    assert_eq!(
        theme.cap(TabStripFocus::Unfocused),
        Palette::DARK.element_strong
    );
    assert_ne!(Palette::DARK.accent, Palette::DARK.element_strong);
}

#[test]
fn animated_look_line_height_delegates_to_text_style() {
    let look = AnimatedLook {
        chrome: Chrome::default(),
        text: TextStyle {
            font_size: 16.0,
            color: RgbaF32::TRANSPARENT,
            line_height_factor: 1.5,
            family: FontFamily::SANS,
            weight: FontWeight::REGULAR,
            slant: FontSlant::Normal,
        },
    };
    assert_eq!(look.text.font().line_height, 24.0);
}

/// A drag value's editor is its chip: the same looks with focus worn as hover, the same box, and
/// the caret, selection and placeholder the theme names; the picker's channel values name the
/// mono face and caption size, and inherit every other text axis.
#[test]
fn a_drag_value_editor_wears_its_chips_looks() {
    use crate::theme::Theme;

    let theme = Theme::from_parts(&Palette::DARK, &Metrics::DEFAULT);
    for value in [&theme.drag_value, &theme.color_picker.value] {
        let editor = value.editor();
        assert_eq!(editor.looks.normal, value.looks.normal);
        assert_eq!(editor.looks.hovered, value.looks.hovered);
        assert_eq!(
            editor.looks.active, value.looks.hovered,
            "focus wears hover"
        );
        assert_eq!(editor.looks.disabled, value.looks.disabled);
        assert_eq!(editor.defaults, value.defaults);
        assert_eq!(
            (
                editor.caret,
                editor.caret_width,
                editor.selection,
                editor.placeholder
            ),
            (
                value.caret,
                value.caret_width,
                value.selection,
                value.placeholder
            )
        );
    }
    assert_ne!(
        theme.drag_value.looks.active, theme.drag_value.looks.hovered,
        "the chip's pressed look is its own, so the mapping shows"
    );
    assert_eq!(
        theme.color_picker.value.looks.normal.text,
        TextStyleOverrides {
            family: Some(FontFamily::MONO),
            font_size: Some(12.0),
            ..TextStyleOverrides::NONE
        },
        "the face and size are the picker's, every other axis the theme's",
    );
}

/// Every bundled text slot past `Theme::text` names at most the color, so size, face and leading follow `Theme::text` even when set after `from_parts`. By-design exceptions, in walk order, which is field order: the picker's mono 12 px values on the value chip and the hex field (four states each) and its mono 12 px captions; the tooltip's 12 px.
#[test]
fn bundled_text_inherits_every_axis_but_color() {
    use crate::theme::{TextSlots, Theme, ThemeText, ThemeTextRef};

    let mono = TextStyleOverrides {
        family: Some(FontFamily::MONO),
        ..TextStyleOverrides::NONE
    };
    let mut expected = vec![mono.with_font_size(12.0); 9];
    expected.push(TextStyleOverrides::NONE.with_font_size(12.0));

    let mut theme = Theme::stock(ColorScheme::Dark);
    let mut named = Vec::new();
    theme.for_each_text(None, &mut |text, _| {
        if let ThemeText::Overrides(o) = text {
            let past_color = TextStyleOverrides { color: None, ..*o };
            if !past_color.is_empty() {
                named.push(past_color);
            }
        }
    });
    assert_eq!(named, expected);
    // The read-only walk visits the same slots, in the same order.
    let mut read = Vec::new();
    theme.each_text(&mut |text| {
        if let ThemeTextRef::Overrides(o) = text {
            let past_color = TextStyleOverrides { color: None, ..*o };
            if !past_color.is_empty() {
                read.push(past_color);
            }
        }
    });
    assert_eq!(read, expected);

    // The reported case: an app at 13 px. An inactive tab names a muted color at rest and none when pressed; both shape at 13 px.
    theme.text = theme.text.with_font_size(13.0);
    let p = Palette::DARK;
    let rest = theme.tabs.unselected.normal.to_animated(theme.text).text;
    let pressed = theme.tabs.unselected.active.to_animated(theme.text).text;
    let disabled = theme.text_edit.looks.disabled.to_animated(theme.text).text;
    assert_eq!(rest, theme.text.with_color(p.text_muted));
    assert_eq!(pressed, theme.text);
    assert_eq!(disabled, theme.text.with_color(p.text_disabled));

    // A menu-bar title rests in the muted ink and takes the full ink lit, hovered or open; a flat
    // button's disabled label is the disabled ink, not the full one.
    let shade = |look: &WidgetLook| look.to_animated(theme.text).text.color;
    let bar = &theme.menu_bar;
    assert_eq!(
        [&bar.title.normal, &bar.title.hovered, &bar.open].map(shade),
        [p.text_muted, p.text, p.text],
    );
    let flat = ButtonTheme::subtle(&p, &Metrics::DEFAULT);
    assert_eq!(shade(&flat.unselected.disabled), p.text_disabled);
    assert_eq!(shade(&flat.unselected.normal), p.text);
}

/// The accent button rests on the accent under `on_accent`; hovered and pressed it moves 12 % and
/// 24 % away from that ink, to white under the dark palette's dark `on_accent` and to black under
/// the light palette's white, so its label's contrast only rises; disabled and selected it is the
/// standard button's.
#[test]
fn the_accent_button_fills_with_the_accent_and_moves_away_from_its_ink() {
    for p in [Palette::DARK, Palette::LIGHT] {
        let stock = ButtonTheme::from_parts(&p, &Metrics::DEFAULT);
        let accent = ButtonTheme::accent(&p, &Metrics::DEFAULT);
        let looks = &accent.unselected;
        let fill = |look: &WidgetLook| look.chrome.fill.as_solid().expect("a solid fill");
        let away = if p == Palette::DARK { 1.0 } else { 0.0 };
        let mixed = |t: f32| {
            RgbaF32::new(
                p.accent.r + (away - p.accent.r) * t,
                p.accent.g + (away - p.accent.g) * t,
                p.accent.b + (away - p.accent.b) * t,
                1.0,
            )
        };
        assert_eq!(
            [&looks.normal, &looks.hovered, &looks.active].map(fill),
            [p.accent, mixed(0.12), mixed(0.24)],
        );
        for look in [&looks.normal, &looks.hovered, &looks.active] {
            assert_eq!(look.text.color, Some(p.on_accent));
            assert_eq!(
                look.chrome.border.color,
                fill(look),
                "no outline of another color"
            );
        }
        let ratio = |look: &WidgetLook| p.on_accent.contrast_ratio(fill(look));
        assert!(ratio(&looks.normal) < ratio(&looks.hovered));
        assert!(ratio(&looks.hovered) < ratio(&looks.active));
        assert_eq!(looks.disabled, stock.unselected.disabled);
        assert_eq!(accent.selected, stock.selected);
        assert_eq!(accent.defaults, stock.defaults);
    }
}
