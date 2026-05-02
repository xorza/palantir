use crate::primitives::{Align, Color, Corners, Stroke, approx_zero};
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
    /// narrower width than the natural unbroken line" (`Wrap`). `align`
    /// positions the glyph bbox inside the owner leaf's arranged rect —
    /// the encoder reads it together with `text_shapes[id].measured` to
    /// shift the emitted `DrawText` rect. `HAlign::Auto`/`Stretch` and
    /// `VAlign::Auto`/`Stretch` collapse to top-left for text (glyphs
    /// don't stretch).
    Text {
        text: String,
        color: Color,
        font_size_px: f32,
        wrap: TextWrap,
        align: Align,
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
                let no_fill = approx_zero(fill.a);
                let no_stroke = match stroke {
                    None => true,
                    Some(s) => approx_zero(s.width) || approx_zero(s.color.a),
                };
                no_fill && no_stroke
            }
            Shape::Line { width, color, .. } => approx_zero(*width) || approx_zero(color.a),
            Shape::Text { text, color, .. } => text.is_empty() || approx_zero(color.a),
        }
    }
}
