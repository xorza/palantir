use crate::input::interaction::button_phase::ButtonPhase;
use crate::input::interaction::button_state::ButtonState;
use crate::input::interaction::response_state::ResponseState;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::text::font_family::FontFamily;
use crate::text::font_slant::FontSlant;
use crate::text::font_weight::FontWeight;
use crate::widget_core::widget_look::WidgetLook;
use crate::widget_core::widget_look::animated_look::AnimatedLook;
use crate::widget_core::widget_look::theme_slot::ThemeSlot;
use crate::widgets::theme::button::ButtonTheme;
use crate::widgets::theme::palette::Palette;
use crate::widgets::theme::text_edit::TextEditTheme;
use crate::widgets::theme::text_style::{TextStyle, TextStyleOverrides};
use crate::widgets::theme::toggle::ToggleTheme;
use std::ptr;

#[test]
fn button_theme_pick_precedence() {
    let theme = ButtonTheme::default();
    let state = |pointer_over, pressed: bool, disabled| ResponseState {
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
    };
    let cases: &[(ResponseState, &WidgetLook, &str)] = &[
        (state(false, false, false), &theme.looks.normal, "normal"),
        (state(true, false, false), &theme.looks.hovered, "hovered"),
        (
            state(true, true, false),
            &theme.looks.active,
            "pressed > hovered",
        ),
        (
            state(false, false, true),
            &theme.looks.disabled,
            "disabled (idle)",
        ),
        (
            state(true, true, true),
            &theme.looks.disabled,
            "disabled wins all",
        ),
    ];
    for (state, expected, label) in cases {
        assert!(
            ptr::eq(theme.look(state, ()), *expected),
            "{label}: look should return the matching slot",
        );
    }
}

#[test]
fn text_edit_theme_pick_precedence() {
    let theme = TextEditTheme::default();
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
}

/// [`ToggleTheme`] is the one `WidgetTheme` whose pick needs an input
/// the response can't supply: `Mode = bool` chooses the look *pack*, and
/// the usual four-state precedence then runs inside it. Both halves are
/// asserted — the pack switching on `checked`, and the state precedence
/// still applying within each — plus that the two packs never resolve to
/// the same slot, which is what makes the `Mode` parameter load-bearing
/// rather than decorative.
#[test]
fn toggle_theme_pick_selects_pack_then_state() {
    let theme = ToggleTheme::checkbox(&Palette::DEFAULT);
    let state = |pointer_over, pressed: bool, disabled| ResponseState {
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
    };
    let cases: &[(ResponseState, bool, &WidgetLook, &str)] = &[
        (
            state(false, false, false),
            false,
            &theme.unchecked.normal,
            "unchecked normal",
        ),
        (
            state(true, false, false),
            false,
            &theme.unchecked.hovered,
            "unchecked hovered",
        ),
        (
            state(false, false, false),
            true,
            &theme.checked.normal,
            "checked normal",
        ),
        (
            state(true, false, false),
            true,
            &theme.checked.hovered,
            "checked hovered",
        ),
        (
            state(true, true, false),
            true,
            &theme.checked.active,
            "checked: pressed > hovered",
        ),
        (
            state(true, true, true),
            true,
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

    // The checked flag decides the answer on its own: one state, two packs.
    let idle = state(false, false, false);
    assert!(
        !ptr::eq(theme.look(&idle, false), theme.look(&idle, true)),
        "checked and unchecked must not resolve to the same look",
    );
}

#[test]
fn animated_look_line_height_delegates_to_text_style() {
    let look = AnimatedLook {
        background: Background::default(),
        text: TextStyle {
            font_size: 16.0,
            color: RgbaF32::TRANSPARENT,
            line_height_mult: 1.5,
            family: FontFamily::SANS,
            weight: FontWeight::REGULAR,
            slant: FontSlant::Normal,
        },
    };
    assert_eq!(look.text.font().line_height, 24.0);
}

/// The picker's channel values keep `DragValueTheme`'s promise: the editor
/// a value turns into is the chip, look for look and padding for padding,
/// so a value that becomes editable keeps its box and its text in place.
/// The picker's text names its face and size and inherits the rest.
#[test]
fn the_picker_value_editor_is_its_chip() {
    use crate::widgets::theme::Theme;

    let theme = Theme::from_palette(&Palette::DEFAULT);
    let value = &theme.color_picker.value;
    assert_eq!(value.editor.looks.normal, value.chip.looks.normal);
    assert_eq!(value.editor.looks.hovered, value.chip.looks.hovered);
    assert_eq!(value.editor.looks.disabled, value.chip.looks.disabled);
    assert_eq!(value.editor.defaults.padding, value.chip.defaults.padding);
    assert_eq!(
        value.chip.looks.normal.text,
        TextStyleOverrides {
            family: Some(FontFamily::MONO),
            font_size: Some(13.0),
            ..TextStyleOverrides::NONE
        },
        "the face and size are the picker's, every other axis the theme's",
    );
}

/// Every bundled text slot past `Theme::text` names the colour at most,
/// so its size, face and leading follow `Theme::text` even when an app
/// sets that after `from_palette`. The exceptions are by design, in walk
/// order: the tooltip's 13 px; the picker's mono 13 px values on the value
/// chip, its editor and the hex field, four states each; and the picker's
/// mono 10 px captions.
#[test]
fn bundled_text_inherits_every_axis_but_colour() {
    use crate::widgets::theme::{Theme, ThemeText};

    let mono = TextStyleOverrides {
        family: Some(FontFamily::MONO),
        ..TextStyleOverrides::NONE
    };
    let mut expected = vec![TextStyleOverrides::NONE.with_font_size(13.0)];
    expected.extend([mono.with_font_size(13.0); 12]);
    expected.push(mono.with_font_size(10.0));

    let mut theme = Theme::default();
    let mut named = Vec::new();
    theme.for_each_text(|text| {
        if let ThemeText::Overrides(o) = text {
            let past_colour = TextStyleOverrides { color: None, ..*o };
            if !past_colour.is_empty() {
                named.push(past_colour);
            }
        }
    });
    assert_eq!(named, expected);

    // The reported case: an app at 13 px. An inactive tab names a muted
    // colour at rest and none when pressed, and both shape at 13 px.
    theme.text = theme.text.with_font_size(13.0);
    let p = Palette::DEFAULT;
    let rest = theme.tabs.inactive.normal.to_animated(theme.text).text;
    let pressed = theme.tabs.inactive.active.to_animated(theme.text).text;
    let disabled = theme.text_edit.looks.disabled.to_animated(theme.text).text;
    assert_eq!(rest, theme.text.with_color(p.text_muted));
    assert_eq!(pressed, theme.text);
    assert_eq!(disabled, theme.text.with_color(p.text_disabled));
}
