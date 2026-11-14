//! The text-run builder; lowers to `ShapeRecord::Text`.

use crate::primitives::layout::align::Align;
use crate::primitives::math::domain::{self, vec2};
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::text::interned_str::InternedStr;
use crate::shape::PaintedBy;
use crate::shape::draw::Draw;
use crate::shape::sealed;
use crate::text::glyph_font::GlyphFont;
use crate::text::wrap::TextWrap;
use glam::Vec2;

/// A shaped text run: its glyphs, font and layout in the owner's rect. Its ink color, straight
/// alpha linear, is its paint ([`Ui::add_shape`](crate::Ui::add_shape)).
#[derive(Clone, Debug)]
#[must_use]
pub struct TextShape {
    /// `None`: the encoder places the glyph bbox in the owner's padded inner rect via `align`. `Some(origin)`: the widget owns positioning (bbox origin is `owner.min + origin`, `align`'s placement axes ignored), as TextEdit does for scroll offsets.
    pub(crate) local_origin: Option<Vec2>,
    pub(crate) text: InternedStr,
    /// The face and metrics to shape in, one named type mirrored by [`ShapeRecord::Text`](crate::shape::record::ShapeRecord::Text).
    pub(crate) font: GlyphFont,
    pub(crate) wrap: TextWrap,
    /// Visual placement and cache-key discriminator: the encoder places the bbox by both axes
    /// (only when `local_origin = None`), and the shaper threads `align.halign()` into cosmic's
    /// alignment and the text cache key.
    pub(crate) align: Align,
}

impl TextShape {
    pub(super) const fn new(text: InternedStr, font: GlyphFont) -> Self {
        Self {
            local_origin: None,
            text,
            font,
            wrap: TextWrap::SingleLine,
            align: Align::TOP_LEFT,
        }
    }

    /// Hand positioning to the caller: the glyph bbox origin becomes `owner.min + origin` and `align`'s placement axes go unread. Named `at_origin`, not `at`, since a run has a pen position rather than a box.
    ///
    /// # Panics
    ///
    /// Panics unless `origin` is an [offset](crate::widget::domain::offset).
    #[track_caller]
    pub const fn at_origin(mut self, origin: Vec2) -> Self {
        self.local_origin = Some(vec2::offset(origin));
        self
    }

    /// Whether the run breaks to the owner's width, and how.
    pub const fn wrap(mut self, wrap: TextWrap) -> Self {
        self.wrap = wrap;
        self
    }

    /// Where the run sits inside its owner. Unread once
    /// [`Self::at_origin`] takes placement over.
    pub const fn align(mut self, a: Align) -> Self {
        self.align = a;
        self
    }
}

impl PaintedBy<RgbaF32> for TextShape {}

/// A text run inked in a color.
impl sealed::DrawWith<RgbaF32> for TextShape {
    type Prepared = RgbaF32;

    fn prepare(paint: RgbaF32) -> RgbaF32 {
        paint
    }

    /// # Panics
    ///
    /// Unless the ink is a [color](crate::widget::domain::color).
    #[track_caller]
    fn check(&self, color: &RgbaF32) {
        let _ = domain::color(*color);
    }

    fn draw<'a>(&'a self, color: &'a RgbaF32) -> Draw<'a> {
        Draw::Text {
            shape: self,
            color: *color,
        }
    }
}
