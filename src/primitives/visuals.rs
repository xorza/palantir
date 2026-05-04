use crate::primitives::{color::Color, stroke::Stroke};

/// One visual state's paint vocabulary. Shared across widget styles
/// (e.g. `ButtonTheme::{normal, hovered, pressed}` are each `Visuals`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Visuals {
    pub fill: Color,
    pub stroke: Option<Stroke>,
    pub text: Color,
}

impl Visuals {
    pub const fn solid(fill: Color, text: Color) -> Self {
        Self {
            fill,
            stroke: None,
            text,
        }
    }
}

impl Default for Visuals {
    fn default() -> Self {
        Self::solid(Color::TRANSPARENT, Color::WHITE)
    }
}
