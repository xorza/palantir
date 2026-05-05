use crate::primitives::color::Color;
use crate::widgets::theme::{Background, TextStyle};

/// One visual state's paint vocabulary: optional background bundle +
/// text style bundle. Shared across widget styles (e.g.
/// `ButtonTheme::{normal, hovered, pressed, disabled}` are each
/// `Visuals`). `background = None` paints nothing for that state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Visuals {
    pub background: Option<Background>,
    pub text: TextStyle,
}

impl Visuals {
    /// Compatibility constructor: solid fill + text color, default
    /// font/leading from `TextStyle::default()`.
    pub fn solid(fill: Color, text_color: Color) -> Self {
        Self {
            background: Some(Background {
                fill,
                ..Background::default()
            }),
            text: TextStyle::default().with_color(text_color),
        }
    }
}

