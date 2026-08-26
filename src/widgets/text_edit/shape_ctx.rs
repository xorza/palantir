//! Everything the shaper is asked for when laying out an editor's text.

use crate::layout::types::align::Align;
use crate::layout::types::align::HAlign;
use crate::primitives::spacing::Spacing;
use crate::text::glyph_font::GlyphFont;
use crate::text::run::TextRun;
use crate::text::wrap::TextWrap;

/// Everything the shaper needs to lay this editor's text out, plus the
/// padding that turns a shaped position into a widget-local one.
///
/// Deliberately carries no block offset: where the shaped block *sits*
/// isn't a shaping input, and holding both here is what let one field
/// mean last frame's offset before the probe and this frame's after it.
/// The two now live apart — [`TextLayout::prev_block_offset`](crate::widgets::text_edit::text_layout::TextLayout::prev_block_offset) and
/// [`TextGeometry::block_offset`](crate::widgets::text_edit::text_geometry::TextGeometry::block_offset).
#[derive(Clone, Copy, Debug)]
pub(super) struct ShapeCtx {
    pub(super) font: GlyphFont,
    pub(super) padding: Spacing,
    wrap_target: Option<f32>,
    pub(super) multiline: bool,
    halign: HAlign,
}

impl ShapeCtx {
    /// The parameters this editor will shape its text with.
    ///
    /// `wrap_target` is the raw inner width a multi-line field wraps to
    /// (`WrapBound::new` owns the canonical rounding) and `None` for a
    /// single-line one; both it and the per-line alignment stay private
    /// because [`Self::run`] is the only thing that reads them, and a
    /// caller writing them directly could disagree with the `TextLayout`
    /// they came from.
    pub(super) fn new(
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

    /// This editor's shaping parameters as the public run description.
    ///
    /// `TextEdit` probes through [`Ui::probe_text`](crate::Ui::probe_text)
    /// like any caller-authored widget would — which is what keeps that
    /// API honest about being enough to build a text widget with.
    ///
    /// A non-multiline editor carries no wrap target, so its `Wrap` /
    /// `SingleLine` choice and its `max_width_px` agree either way: both
    /// resolve to an unbounded shape.
    pub(super) fn run<'a>(&self, text: &'a str) -> TextRun<'a> {
        TextRun {
            text,
            font: self.font,
            wrap: if self.multiline {
                TextWrap::Wrap
            } else {
                TextWrap::SingleLine
            },
            align: Align::h(self.halign),
            max_width_px: self.wrap_target,
        }
    }
}
