use crate::primitives::{ApproxF32, Color, Corners, Rect, Stroke};
use glam::Vec2;

/// Where a shape sits inside its owner Node.
/// Resolved against `Node.rect` at paint time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShapeRect {
    /// Fill the owner's full arranged rect.
    Full,
    /// Offset relative to the owner's `rect.min`.
    Offset(Rect),
}

#[derive(Clone, Debug)]
pub enum Shape {
    RoundedRect {
        bounds: ShapeRect,
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
    /// Placeholder until glyphon is wired up. `measured` is the pre-shaped run size
    /// so layout can ask for it.
    Text {
        offset: Vec2,
        text: String,
        color: Color,
        measured: crate::primitives::Size,
    },
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
