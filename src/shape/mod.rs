use crate::primitives::{ApproxF32, Color, Corners, Size, Stroke};
use crate::text::TextCacheKey;
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
    /// Shaped text run. `measured` is the pre-shaped bounding size used by
    /// the measure pass; `key` identifies the shaped `cosmic_text::Buffer`
    /// in the active [`crate::text::TextMeasure`] so the renderer can look
    /// it up without reshaping. Runs whose `key` is
    /// [`TextCacheKey::INVALID`] (e.g. produced by `MonoMeasure`) are
    /// dropped at render time — the size still drives layout.
    Text {
        offset: Vec2,
        text: String,
        color: Color,
        measured: Size,
        font_size_px: f32,
        max_width_px: Option<f32>,
        key: TextCacheKey,
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
