use crate::widgets::ButtonStyle;

/// Default `ButtonStyle` for buttons that don't supply one. Read by
/// `Button::show` when `Button::style` was not called. Other widgets
/// (Frame / Panel / Grid) take their visuals at builder time via the
/// `Styled` mixin and don't consult this — there's no global theme;
/// "theme" today means "button defaults" only.
#[derive(Clone, Debug, Default)]
pub struct ButtonTheme {
    pub button: ButtonStyle,
}
