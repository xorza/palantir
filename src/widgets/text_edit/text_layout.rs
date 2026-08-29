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
