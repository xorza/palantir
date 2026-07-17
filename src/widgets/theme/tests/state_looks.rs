use crate::input::response::{ButtonPhase, ButtonState, ResponseState};
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::text::{FontFamily, FontWeight};
use crate::widgets::theme::button::ButtonTheme;
use crate::widgets::theme::text_edit::TextEditTheme;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::theme::widget_look::{AnimatedLook, WidgetLook};

#[test]
fn button_theme_pick_precedence() {
    let theme = ButtonTheme::default();
    let state = |hovered, pressed: bool, disabled| ResponseState {
        hovered,
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
            std::ptr::eq(theme.pick(*state), *expected),
            "{label}: pick should return the matching slot",
        );
    }
}

#[test]
fn text_edit_theme_pick_precedence() {
    let theme = TextEditTheme::default();
    let state = |focused, hovered, disabled| ResponseState {
        disabled,
        focused,
        hovered,
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
            std::ptr::eq(theme.pick(*state), *expected),
            "{label}: pick should return the matching slot",
        );
    }
}

#[test]
fn animated_look_line_height_px_delegates_to_text_style() {
    let look = AnimatedLook {
        background: Background::default(),
        text: TextStyle {
            font_size_px: 16.0,
            color: Color::TRANSPARENT,
            line_height_mult: 1.5,
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
        },
    };
    assert!((look.line_height_px() - 24.0).abs() < 1e-6);
}
