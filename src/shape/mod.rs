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
    /// Shaped text run. `measured` is the unbounded shape's bounding size
    /// recorded at `show()` time; `key` identifies the shaped
    /// `cosmic_text::Buffer` so the renderer can look it up without
    /// reshaping. Runs whose `key` is [`TextCacheKey::INVALID`] (e.g.
    /// produced by `mono_measure`) are dropped at render time — the size
    /// still drives layout.
    ///
    /// `wrap` selects between "shape once and freeze" (`Single`) and
    /// "reshape if the parent commits a narrower width" (`Wrap`). When the
    /// measure pass commits a narrower width to a `Wrap` shape, it stores
    /// the reshaped size + `key` on `LayoutResult.text_reshapes` keyed by
    /// `NodeId` — the recorded shape stays untouched so `Tree` is read-only
    /// during layout. `max_width_px` is the user-requested cap (today
    /// always `None`, kept for diagnostics + future authoring ergonomics).
    Text {
        // todo review fields
        offset: Vec2,
        text: String,
        color: Color,
        measured: Size,
        font_size_px: f32,
        max_width_px: Option<f32>,
        key: TextCacheKey,
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
    /// Allow the arrange pass to reshape this run at the committed width.
    /// `intrinsic_min` is the width of the widest unbreakable run (longest
    /// word), measured at shape time and used as the floor when the parent
    /// commits a narrower width — the run overflows rather than breaking
    /// inside a word.
    Wrap { intrinsic_min: f32 },
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
