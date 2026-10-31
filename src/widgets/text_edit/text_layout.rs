//! What is known about an editor's text box before the shape probe runs.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::layout::align::Align;
use crate::text::glyph_font::GlyphFont;
use crate::widgets::text_edit::shape_ctx::ShapeCtx;
use glam::Vec2;

#[derive(Clone, Copy, Debug)]
pub(super) struct LayoutInput {
    pub(super) response_rect: Option<Rect>,
    pub(super) padding: Spacing,
    pub(super) caret_width: f32,
    pub(super) font: GlyphFont,
    pub(super) multiline: bool,
    pub(super) text_align: Option<Align>,
    pub(super) prev_block_offset: Vec2,
}

/// What is known before the shape probe runs: the text box and its shaping parameters. The input pass hit-tests against this (last frame's layout); probe output lands in [`TextGeometry`](crate::widgets::text_edit::text_geometry::TextGeometry).
#[derive(Clone, Copy, Debug)]
pub(super) struct TextLayout {
    pub(super) ctx: ShapeCtx,
    pub(super) text_align: Align,
    /// The caret's drawn width, clamped; the room the field reserves by.
    pub(super) caret_room: f32,
    /// The box the text is measured and scrolled inside: the field's rect less padding; `None` before it is arranged.
    pub(super) inner: Option<Rect>,
    /// Where the shaped block sat when last painted; a click this frame was aimed at that layout, so hit-testing offsets by this.
    pub(super) prev_block_offset: Vec2,
}

impl TextLayout {
    /// The alignment the block is placed by; a multi-line field aligns vertically and lets the shaper align each line. Read by both the layout engine and the record pass's hit-test.
    pub(super) const fn block_align(&self) -> Align {
        if self.ctx.multiline {
            Align::v(self.text_align.valign())
        } else {
            self.text_align
        }
    }

    /// The box the block occupies for what is on show: floored at one line, widened on a single line by the caret's room so an end-of-text caret falls inside the block (a wrapped block reserves none). Uses the shaper's line height, not the theme's leading, since it is quantized to 1/64 px.
    pub(super) fn block_size(&self, display: Size) -> Size {
        let room = if self.ctx.multiline {
            0.0
        } else {
            self.caret_room
        };
        Size::new(display.w + room, display.h.max(self.ctx.font.line_height))
    }

    /// Room a single line keeps for the caret past its glyphs at both ends; none for a wrapped block.
    pub(super) const fn caret_reserve(&self) -> f32 {
        if self.ctx.multiline {
            0.0
        } else {
            single_line_caret_reserve(self.caret_room)
        }
    }

    /// [`Self::inner`]'s extent, collapsing the unarranged frame to nothing (sizing math; the scroll view wants the absence).
    pub(super) fn inner_size(&self) -> Size {
        self.inner.map_or(Size::ZERO, |rect| rect.size)
    }

    /// Resolve the text box and shaping parameters from the field's rect, padding, and font.
    pub(super) fn resolve(input: LayoutInput) -> Self {
        let caret_room = caret_room(input.caret_width);
        // One deflation, so the wrap width and the measured box cannot disagree (a raw subtraction would commit a negative wrap width on an over-constrained field).
        let inner = input
            .response_rect
            .map(|rect| rect.deflated_by(input.padding));
        let text_align = input.text_align.unwrap_or(if input.multiline {
            Align::TOP_LEFT
        } else {
            Align::LEFT
        });
        Self {
            ctx: ShapeCtx::new(&input, inner, text_align.halign()),
            text_align,
            caret_room,
            inner,
            prev_block_offset: input.prev_block_offset,
        }
    }
}

/// The room a caret of `caret_width` takes past the text's end, a negative width taking none.
pub(crate) const fn caret_room(caret_width: f32) -> f32 {
    caret_width.max(0.0)
}

/// What a single-line field adds to its text's width for the caret at either end, all of it after
/// left-aligned text: the width an [`EditableLabel`](crate::EditableLabel)'s idle label reserves
/// so the glyphs hold their place when it swaps to its editor.
pub(crate) const fn single_line_caret_reserve(caret_room: f32) -> f32 {
    2.0 * caret_room
}
