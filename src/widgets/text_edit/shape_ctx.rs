//! Everything the shaper is asked for when laying out an editor's text.

use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::layout::align::Align;
use crate::primitives::layout::align::HAlign;
use crate::text::glyph_font::GlyphFont;
use crate::text::run::TextRun;
use crate::text::wrap::TextWrap;

/// What the shaper needs to lay out this editor's text, plus the padding turning a shaped position widget-local.
///
/// No block offset: it isn't a shaping input, and one field once meant last frame's offset before the probe and this frame's after. See [`TextLayout::prev_block_offset`](crate::widgets::text_edit::text_layout::TextLayout::prev_block_offset) and [`TextGeometry::block_offset`](crate::widgets::text_edit::text_geometry::TextGeometry::block_offset).
#[derive(Clone, Copy, Debug)]
pub(super) struct ShapeCtx {
    pub(super) font: GlyphFont,
    pub(super) padding: Spacing,
    wrap_target: Option<f32>,
    pub(super) multiline: bool,
    halign: HAlign,
}

impl ShapeCtx {
    /// The parameters this editor shapes with. `wrap_target` is the raw inner width of a multi-line field (`WrapBound::new` rounds) and `None` for single-line; both it and per-line alignment are private since only [`Self::run`] reads them.
    pub(super) const fn new(
        font: GlyphFont,
        padding: Spacing,
        wrap_target: Option<f32>,
        multiline: bool,
        halign: HAlign,
    ) -> Self {
        Self {
            font,
            padding,
            wrap_target,
            multiline,
            halign,
        }
    }

    /// This editor's shaping parameters as the public run description. `TextEdit` probes through [`Ui::probe_text`](crate::Ui::probe_text) like any caller widget. A non-multiline editor has no wrap target, so `Wrap` / `SingleLine` and `max_width` both resolve unbounded.
    pub(super) const fn run<'a>(&self, text: &'a str) -> TextRun<'a> {
        TextRun {
            text,
            font: self.font,
            wrap: if self.multiline {
                TextWrap::Wrap
            } else {
                TextWrap::SingleLine
            },
            align: Align::h(self.halign),
            max_width: self.wrap_target,
        }
    }
}
