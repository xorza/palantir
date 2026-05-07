use crate::layout::types::align::Align;
use crate::primitives::{
    approx::approx_zero, color::Color, corners::Corners, rect::Rect, stroke::Stroke,
};
use glam::Vec2;
use std::borrow::Cow;
use std::hash::{Hash, Hasher};

#[derive(Clone, Debug)]
pub enum Shape {
    /// Filled/stroked rounded rectangle. With `local_rect = None` it covers
    /// the owner node's full arranged rect (position/size come from layout).
    /// With `local_rect = Some(r)` it paints `r` at owner-relative coords —
    /// `r.min = (0, 0)` is the owner's top-left. The sub-rect form paints in
    /// the slot it was pushed in (interleaved with children via the slot
    /// mechanism — see `Tree::add_shape`), still under the owner's clip but
    /// outside its pan transform. Used for scrollbar tracks/thumbs (pushed
    /// after body content → slot N) and TextEdit carets (pushed after the
    /// Text shape on a leaf → slot 0, after the Text in record order).
    RoundedRect {
        local_rect: Option<Rect>,
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
    /// positions the glyph bbox inside the owner leaf's arranged rect (or
    /// `local_rect` if set) — the encoder reads it together with the
    /// shaped run's `measured` to shift the emitted `DrawText` rect.
    /// `HAlign::Auto`/`Stretch` and `VAlign::Auto`/`Stretch` collapse to
    /// top-left for text (glyphs don't stretch).
    ///
    /// `local_rect` mirrors `RoundedRect::local_rect`: `None` paints into
    /// the owner's arranged rect (deflated by the node's `padding`);
    /// `Some(lr)` paints `lr` at owner-relative coords (`lr.min = (0, 0)`
    /// is owner top-left), with `padding` skipped and `align` positioning
    /// the run *inside `lr`*. Lets a custom widget place multiple text
    /// runs in one leaf without each clobbering the others.
    Text {
        local_rect: Option<Rect>,
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

impl Hash for Shape {
    /// Discriminant tags are stable (`RoundedRect=0`, `Line=1`, `Text=2`) so
    /// cache keys don't shift if variants are reordered.
    fn hash<H: Hasher>(&self, h: &mut H) {
        match self {
            Shape::RoundedRect {
                local_rect,
                radius,
                fill,
                stroke,
            } => {
                h.write_u8(0);
                match local_rect {
                    None => h.write_u8(0),
                    Some(r) => {
                        h.write_u8(1);
                        r.hash(h);
                    }
                }
                radius.hash(h);
                fill.hash(h);
                match stroke {
                    None => h.write_u8(0),
                    Some(s) => {
                        h.write_u8(1);
                        s.hash(h);
                    }
                }
            }
            Shape::Line { a, b, width, color } => {
                h.write_u8(1);
                h.write(bytemuck::bytes_of(a));
                h.write(bytemuck::bytes_of(b));
                h.write_u32(width.to_bits());
                color.hash(h);
            }
            Shape::Text {
                local_rect,
                text,
                color,
                font_size_px,
                line_height_px,
                wrap,
                align,
            } => {
                h.write_u8(2);
                match local_rect {
                    None => h.write_u8(0),
                    Some(r) => {
                        h.write_u8(1);
                        r.hash(h);
                    }
                }
                text.hash(h);
                color.hash(h);
                h.write_u32(font_size_px.to_bits());
                h.write_u32(line_height_px.to_bits());
                h.write_u16(((align.raw() as u16) << 8) | *wrap as u8 as u16);
            }
        }
    }
}

/// True iff `local_rect` is set and has zero width or height. Shared
/// between `RoundedRect`/`Text` `is_noop` arms — `None` means
/// "paint into owner's full rect", which is never zero-area.
#[inline]
fn local_rect_zero_area(local_rect: &Option<Rect>) -> bool {
    local_rect
        .map(|r| approx_zero(r.size.w) || approx_zero(r.size.h))
        .unwrap_or(false)
}

impl Shape {
    /// True if this shape paints nothing visible (transparent fill + no stroke,
    /// zero-width line, empty text, etc.). `Ui::add_shape` filters these out so
    /// widgets can push speculatively without guarding.
    pub fn is_noop(&self) -> bool {
        match self {
            Shape::RoundedRect {
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
                local_rect_zero_area(local_rect) || (no_fill && no_stroke)
            }
            Shape::Line { width, color, .. } => approx_zero(*width) || approx_zero(color.a),
            Shape::Text {
                text,
                color,
                local_rect,
                ..
            } => local_rect_zero_area(local_rect) || text.is_empty() || approx_zero(color.a),
        }
    }
}
