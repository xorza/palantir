//! Describing a run to probe: the input half of the public text-geometry
//! surface.
//!
//! Layout already measures and paints text for any widget that records a
//! [`Shape::Text`](crate::Shape) — nothing here is needed for that. What
//! this and [`probe`](crate::text::probe) add is the other direction:
//! mapping between byte offsets and positions inside a run, which is what
//! a widget needs to place a caret, turn a click into an offset, or paint
//! a selection.

use crate::layout::types::align::Align;
use crate::text::glyph_font::GlyphFont;
use crate::text::request::TextShapeRequest;
use crate::text::wrap::TextWrap;

/// One text run, described the way [`Shape::Text`](crate::Shape)
/// describes one.
///
/// **The spelling mirrors `Shape::Text` on purpose.** A probe that
/// describes a different run than the paint puts the caret in the wrong
/// place, and that is invisible until someone reports their cursor
/// sitting slightly off in bold text — so the two are written the same
/// way, field for field, and a mismatch reads as one in review.
///
/// The paint-only fields are absent (`color`, `local_origin`): they move
/// glyphs on screen but never change how the run is shaped, so they
/// cannot change any answer here.
#[derive(Clone, Copy, Debug)]
pub struct TextRun<'a> {
    pub text: &'a str,
    /// The face and metrics the run is shaped in — the same
    /// [`GlyphFont`] `Shape::Text` carries, so describing a probe means
    /// naming one value rather than restating four.
    pub font: GlyphFont,
    pub wrap: TextWrap,
    /// Only the horizontal half is read — cosmic lays out per-line `x`
    /// offsets from it, so it changes the shaped result. The vertical
    /// half places the block within its owner and is the encoder's
    /// business, exactly as in `Shape::Text`.
    pub align: Align,
    /// The width the run is shaped against, or `None` for unbounded.
    ///
    /// The one field `Shape::Text` has no counterpart for: a painted run
    /// gets its width from the arranged rect, which does not exist until
    /// layout has run. A probe has to say which width it means — pass
    /// the inner width the run will be (or was) laid out in.
    ///
    /// Ignored for the [`TextWrap`] policies that always keep their
    /// unbounded shape, so passing a width to a `SingleLine` run is not
    /// a mistake, just inert.
    pub max_width_px: Option<f32>,
}

impl<'a> TextRun<'a> {
    /// Lower to the shaper's *unbounded* request — the run's root, before
    /// any width is bound to it.
    ///
    /// Binding is deliberately not done here. Which width layout actually
    /// commits depends on the root itself: a truncating fit whose text
    /// already fits keeps the unbounded buffer, and `WrapWithOverflow`
    /// raises a too-narrow width to the root's wrap floor. Both need a
    /// shaping call, so [`TextShaper::layout`](crate::TextShaper) applies
    /// them and this stays the part that needs no shaper.
    pub(crate) fn unbounded_request(&self) -> TextShapeRequest<'a> {
        TextShapeRequest::unbounded(self.text, self.font)
    }
}
