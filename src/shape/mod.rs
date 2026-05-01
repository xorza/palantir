use crate::primitives::{ApproxF32, Color, Corners, Stroke};
use glam::Vec2;

#[derive(Clone, Debug)]
pub enum Shape {
    /// Filled/stroked rounded rectangle covering the owner node's arranged
    /// rect. Position and size come from the node — shapes don't carry their
    /// own bounds.
    RoundedRect {
        radius: Corners,
        fill: Color,
        stroke: Option<Stroke>,
    },
    Line {
        a: Vec2,
        b: Vec2,
        width: f32,
        color: Color,
    },
    /// Shaped text run — *authoring inputs only*. Measured size and
    /// shaped-buffer key are layout outputs and live on
    /// `LayoutResult.text_shapes`, not here. `wrap` selects between "shape
    /// once and freeze" (`Single`) and "reshape if the parent commits a
    /// narrower width than the natural unbroken line" (`Wrap`).
    Text {
        text: String,
        color: Color,
        font_size_px: f32,
        wrap: TextWrap,
    },
}

/// Wrap mode for [`Shape::Text`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TextWrap {
    /// Shape once at unbounded width and never reshape. Used by every text
    /// run that fits on a single line — labels, headings, anything that
    /// shouldn't wrap.
    Single,
    /// Reshape during measure if the parent commits a width narrower than
    /// the natural unbroken line. The widest unbreakable run (longest word)
    /// is the floor — text overflows rather than breaking inside a word.
    Wrap,
}

impl Shape {
    /// True if this shape paints nothing visible (transparent fill + no stroke,
    /// zero-width line, empty text, etc.). `Ui::add_shape` filters these out so
    /// widgets can push speculatively without guarding.
    pub fn is_noop(&self) -> bool {
        match self {
            Shape::RoundedRect { fill, stroke, .. } => {
                let no_fill = fill.a.approx_zero();
                let no_stroke = match stroke {
                    None => true,
                    Some(s) => s.width.approx_zero() || s.color.a.approx_zero(),
                };
                no_fill && no_stroke
            }
            Shape::Line { width, color, .. } => width.approx_zero() || color.a.approx_zero(),
            Shape::Text { text, color, .. } => text.is_empty() || color.a.approx_zero(),
        }
    }
}
