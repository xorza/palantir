//! Describing a run to probe: the input half of the public text-geometry
//! surface. Layout already measures and paints [`Shape::Text`](crate::widget::Shape);
//! this and [`probe`](crate::text::probe) add the other direction, mapping byte
//! offsets to positions inside a run, for carets, clicks and selections.

use crate::primitives::layout::align::Align;
use crate::text::glyph_font::GlyphFont;
use crate::text::key::TextShapeKey;
use crate::text::request::TextShapeRequest;
use crate::text::wrap::TextWrap;

/// One text run, described as [`Shape::Text`](crate::widget::Shape) describes one.
///
/// **The spelling mirrors `Shape::Text` on purpose:** a probe describing a
/// different run than the paint misplaces the caret invisibly, so the fields
/// match one for one. Paint-only fields (`color`, `local_origin`) are absent;
/// they never change shaping.
#[derive(Clone, Copy, Debug)]
pub struct TextRun<'a> {
    /// The characters to shape.
    pub text: &'a str,
    /// The face and metrics the run is shaped in: the [`GlyphFont`] `Shape::Text`
    /// carries.
    pub font: GlyphFont,
    /// Whether the run breaks to the shaping width, and how.
    pub wrap: TextWrap,
    /// Only the horizontal half is read (cosmic lays out per-line `x` from it); the
    /// vertical half places the block in its owner, the encoder's business.
    pub align: Align,
    /// The width the run is shaped against, or `None` for unbounded.
    ///
    /// The one field `Shape::Text` lacks: a painted run's width comes from the
    /// arranged rect, so a probe must say which width it means (the inner width the
    /// run is laid out in).
    ///
    /// Inert for [`TextWrap`] policies that keep their unbounded shape (e.g.
    /// `SingleLine`) and for a non-finite width.
    pub max_width: Option<f32>,
}

impl<'a> TextRun<'a> {
    /// Lower to the shaper's *unbounded* request, the run's root before any width is
    /// bound. `None` for nothing to shape (no bytes, or a face with no usable size);
    /// [`TextShaper::layout`](crate::TextShaper) answers that with an empty probe.
    ///
    /// Binding isn't done here: the committed width depends on the root (a truncating
    /// fit that already fits keeps the unbounded buffer; `WrapWithOverflow` raises a
    /// narrow width to the wrap floor), which needs a shaping call, so
    /// [`TextShaper::layout`](crate::TextShaper) applies it.
    pub(crate) fn unbounded_request(&self) -> Option<TextShapeRequest<'a>> {
        TextShapeRequest::unbounded(self.text, self.font)
    }

    /// The key this run's unbounded shape is cached under, even with nothing to
    /// shape: the probe's metrics live on it. `None` for a face the shaper can't be
    /// asked for, where a probe reports zero line height.
    pub(crate) fn unbounded_key(&self) -> Option<TextShapeKey> {
        TextShapeKey::for_text(self.text, self.font)
    }

    /// The width this run binds to, or `None`. [`Self::max_width`] is filled from a
    /// caller's arithmetic, so "no width" arrives as absent or non-finite; both keep
    /// the unbounded shape, and answering here keeps a non-finite width out of
    /// `WrapBound`'s quantization.
    pub(crate) fn wrap_width(&self) -> Option<f32> {
        self.max_width.filter(|width| width.is_finite())
    }
}
