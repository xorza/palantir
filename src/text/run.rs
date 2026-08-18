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
use crate::text::key::WrapBound;
use crate::text::request::TextShapeRequest;
use crate::text::wrap::TextWrap;
use crate::text::{FontFamily, FontWeight};

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
    pub font_size_px: f32,
    pub line_height_px: f32,
    pub wrap: TextWrap,
    /// Only the horizontal half is read — cosmic lays out per-line `x`
    /// offsets from it, so it changes the shaped result. The vertical
    /// half places the block within its owner and is the encoder's
    /// business, exactly as in `Shape::Text`.
    pub align: Align,
    pub family: FontFamily,
    pub weight: FontWeight,
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
    /// Lower to the shaper's request.
    ///
    /// Bounded exactly when layout would bind it — same
    /// [`TextWrap::line_fit`] mapping, same `halign` — so probing a run
    /// hits the buffer the paint shaped rather than minting a second one
    /// that answers slightly differently.
    pub(crate) fn request(&self) -> TextShapeRequest<'a> {
        let request = TextShapeRequest::unbounded(
            self.text,
            self.font_size_px,
            self.line_height_px,
            self.family,
            self.weight,
        );
        match (self.max_width_px, self.wrap.line_fit()) {
            (Some(width), Some(fit)) => {
                request.with_bound(WrapBound::new(width, self.align.halign(), fit))
            }
            _ => request,
        }
    }
}
