//! What is known about an editor's text box before the shape probe runs.

use crate::layout::types::align::Align;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::text::glyph_font::GlyphFont;
use crate::widgets::text_edit::shape_ctx::ShapeCtx;
use glam::Vec2;

#[derive(Debug)]
pub(super) struct LayoutInput {
    pub(super) response_rect: Option<Rect>,
    pub(super) padding: Spacing,
    pub(super) caret_width: f32,
    pub(super) font: GlyphFont,
    pub(super) multiline: bool,
    pub(super) text_align: Option<Align>,
    pub(super) previous_block_offset: Vec2,
}

/// What is known **before** the shape probe runs: the box the text sits
/// in and the parameters it will be shaped with.
///
/// The input pass reads this — it hit-tests a click against the layout
/// the user was looking at, which is last frame's. Everything the probe
/// itself produces lands in [`TextGeometry`](crate::widgets::text_edit::text_geometry::TextGeometry) instead, so neither type
/// ever holds a field that isn't answered yet.
#[derive(Clone, Copy, Debug)]
pub(super) struct TextLayout {
    pub(super) ctx: ShapeCtx,
    pub(super) text_align: Align,
    /// The caret's drawn width, clamped — the one value the field
    /// reserves room by. Read through [`Self::block_size`] and
    /// [`Self::caret_reserve`], which are the two questions it answers.
    pub(super) caret_room: f32,
    /// The box the text is measured and scrolled inside — the field's
    /// rect less its padding. `None` before the field has been arranged,
    /// which is the same question `response_rect` answered and the reason
    /// nothing downstream re-derives it.
    pub(super) inner: Option<Rect>,
    /// Where the shaped block sat when it was last painted. The click
    /// that arrives this frame was aimed at *that* layout, so the
    /// hit-test in `input` offsets by this rather than by the offset
    /// this frame's probe is about to produce.
    pub(super) prev_block_offset: Vec2,
}

impl TextLayout {
    /// The alignment the block is placed by, on the axes the field
    /// aligns: a multi-line field aligns its block vertically and lets the
    /// shaper align each line inside it.
    ///
    /// One definition because both ends of the placement read it — the
    /// block node hands it to the layout engine, and the record pass
    /// undoes it to hit-test a click against last frame's placement.
    pub(super) fn block_align(&self) -> Align {
        if self.ctx.multiline {
            Align::v(self.text_align.valign())
        } else {
            self.text_align
        }
    }

    /// The box the block occupies for what is on show.
    ///
    /// Floored at one line so an empty field still has a caret's worth of
    /// height to stand up in, and widened on a single line by the caret's
    /// room so a caret at the end of the text falls *inside* the block it
    /// belongs to instead of just past it. A wrapped block reserves
    /// nothing — its caret has a next line to fall to.
    ///
    /// The shaper's line height rather than the theme's leading: the two
    /// differ in the last thousandth of a pixel — the shaped one is
    /// quantized to 1/64 px — and the field's box has to agree with the
    /// run inside it.
    pub(super) fn block_size(&self, display: Size) -> Size {
        let room = if self.ctx.multiline {
            0.0
        } else {
            self.caret_room
        };
        Size::new(
            display.w + room,
            display.h.max(self.ctx.font.line_height_px),
        )
    }

    /// Room a single line keeps for the caret past its glyphs, at both
    /// ends: what the view can pan to, and what a Hug field reserves so it
    /// never has to. A wrapped block reserves none, for the same reason
    /// [`Self::block_size`] does not.
    pub(super) fn caret_reserve(&self) -> f32 {
        if self.ctx.multiline {
            0.0
        } else {
            2.0 * self.caret_room
        }
    }

    /// [`Self::inner`]'s extent, collapsing the unarranged frame to
    /// nothing — what the sizing math wants, where the scroll view wants
    /// the absence itself.
    pub(super) fn inner_size(&self) -> Size {
        self.inner.map_or(Size::ZERO, |rect| rect.size)
    }

    /// Resolve the box the text sits in and the parameters it will be
    /// shaped with, from the field's rect, padding, and font.
    pub(super) fn resolve(input: LayoutInput) -> Self {
        let caret_room = input.caret_width.max(0.0);
        // One deflation, so the width the text wraps at and the box it
        // is measured against cannot disagree. Spelled apart, the wrap
        // target keeps a raw subtraction where the measured box clamps,
        // and an over-constrained field commits a negative wrap width —
        // the case `canonical_wrap_width`'s own clamp catches one layer
        // further down.
        let inner = input
            .response_rect
            .map(|rect| rect.deflated_by(input.padding));
        let wrap_target = inner.filter(|_| input.multiline).map(|rect| rect.size.w);
        let text_align = input.text_align.unwrap_or(if input.multiline {
            Align::TOP_LEFT
        } else {
            Align::LEFT
        });
        let ctx = ShapeCtx::new(
            input.font,
            input.padding,
            wrap_target,
            input.multiline,
            text_align.halign(),
        );
        TextLayout {
            ctx,
            text_align,
            caret_room,
            inner,
            prev_block_offset: input.previous_block_offset,
        }
    }
}
