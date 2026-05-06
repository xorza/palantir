use crate::layout::types::align::Align;
use crate::primitives::{
    approx::approx_zero, color::Color, corners::Corners, rect::Rect, stroke::Stroke,
};
use glam::Vec2;
use std::borrow::Cow;

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
    /// Filled/stroked rounded rect at an explicit owner-relative
    /// sub-rect. `local_rect.min` is `(0, 0)` at the owner's top-left
    /// corner. Paints in the slot the shape was pushed in (interleaved
    /// with children via the slot mechanism — see `Tree::add_shape`),
    /// still under the owner's clip but outside its pan transform. Used
    /// for scrollbar tracks/thumbs (pushed after body content → slot N)
    /// and TextEdit carets (pushed after the Text shape on a leaf → slot
    /// 0, after the Text in record order).
    SubRect {
        local_rect: Rect,
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
        /// `Cow<'static, str>` so static-string labels (the common case via
        /// `&'static str → Into<Cow<…>>`) round-trip with only pointer-copy
        /// `Clone`s — no per-frame heap alloc. Dynamic strings still allocate
        /// once into `Cow::Owned` at the authoring boundary.
        text: Cow<'static, str>,
        color: Color,
        font_size_px: f32,
        /// Line-height in logical px, fed straight to the shaper's
        /// `Metrics::new`. Authoring-side widgets typically set this to
        /// `font_size_px * line_height_mult` where the multiplier
        /// defaults to [`crate::text::LINE_HEIGHT_MULT`] (1.2). Carrying
        /// the resolved px on the shape — instead of a multiplier the
        /// shaper would re-resolve — means the shaper doesn't have to
        /// know about widget conventions, and two `Shape::Text` runs at
        /// the same font-size but different leading correctly produce
        /// distinct cached shaped buffers (via [`TextCacheKey::lh_q`]).
        line_height_px: f32,
        wrap: TextWrap,
        align: Align,
    },
}

/// Wrap mode for [`Shape::Text`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
            Shape::SubRect {
                local_rect,
                fill,
                stroke,
                ..
            } => {
                let no_fill = approx_zero(fill.a);
                let no_stroke = match stroke {
                    None => true,
                    Some(s) => approx_zero(s.width) || approx_zero(s.color.a),
                };
                let zero_area = approx_zero(local_rect.size.w) || approx_zero(local_rect.size.h);
                zero_area || (no_fill && no_stroke)
            }
            Shape::Line { width, color, .. } => approx_zero(*width) || approx_zero(color.a),
            Shape::Text { text, color, .. } => text.is_empty() || approx_zero(color.a),
        }
    }
}
