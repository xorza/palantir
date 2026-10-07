//! The text-run builder; lowers to `ShapeRecord::Text`.

use crate::primitives::layout::align::Align;
use crate::primitives::math::domain::{self, vec2};
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::text::interned_str::InternedStr;
use crate::scene::record_store::RecordStore;
use crate::shape::record::ShapeRecord;
use crate::shape::sealed;
use crate::text::font_family::FontFamily;
use crate::text::font_slant::FontSlant;
use crate::text::font_weight::FontWeight;
use crate::text::glyph_font::GlyphFont;
use crate::text::wrap::TextWrap;
use glam::Vec2;

/// Shaped text run owned by the active node.
#[derive(Clone, Debug)]
#[must_use]
pub struct TextShape {
    /// `None`: the encoder places the glyph bbox in the owner's padded inner rect via `align`. `Some(origin)`: the widget owns positioning (bbox origin is `owner.min + origin`, `align`'s placement axes ignored), as TextEdit does for scroll offsets.
    pub(crate) local_origin: Option<Vec2>,
    pub(crate) text: InternedStr,
    pub(crate) color: RgbaF32,
    /// The face and metrics to shape in, one named type mirrored by [`ShapeRecord::Text`].
    pub(crate) font: GlyphFont,
    pub(crate) wrap: TextWrap,
    /// Visual placement and cache-key discriminator: the encoder places the bbox by both axes (only when `local_origin = None`), and layout threads `align.halign()` into cosmic's `set_align` and the text cache key.
    pub(crate) align: Align,
}

impl TextShape {
    pub(super) const fn new(text: InternedStr, font: GlyphFont) -> Self {
        Self {
            local_origin: None,
            text,
            color: RgbaF32::WHITE,
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
}
impl TextShape {
    /// Ink colour: straight-alpha linear RGB.
    ///
    /// # Panics
    ///
    /// Panics unless `color` is a [colour](crate::widget::domain::color).
    #[track_caller]
    pub const fn color(mut self, color: RgbaF32) -> Self {
        self.color = domain::color(color);
        self
    }

    /// Whether the run breaks to the owner's width, and how.
    pub const fn wrap(mut self, wrap: TextWrap) -> Self {
        self.wrap = wrap;
        self
    }

    /// Where the run sits inside its owner. Unread once
    /// [`Self::at_origin`] takes placement over.
    pub const fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    /// Font family.
    pub const fn family(mut self, family: FontFamily) -> Self {
        self.font.family = family;
        self
    }

    /// Which weight to shape at, on the CSS 1–1000 scale.
    pub const fn weight(mut self, weight: FontWeight) -> Self {
        self.font.weight = weight;
        self
    }

    /// Font slant.
    pub const fn slant(mut self, slant: FontSlant) -> Self {
        self.font.slant = slant;
        self
    }
}

impl sealed::LowerShape for TextShape {
    /// An unusable face shapes nothing, as the two public text queries answer too.
    fn is_noop(&self) -> bool {
        self.text.is_empty() || self.color.is_noop() || !self.font.metrics_valid()
    }

    /// `font` is not asked: `metrics_valid` in `is_noop` is stricter.
    fn has_nan(&self) -> bool {
        self.local_origin.has_nan() || self.color.has_nan()
    }

    fn lower(self, store: &mut RecordStore) -> ShapeRecord {
        let Self {
            local_origin,
            text,
            color,
            font,
            wrap,
            align,
        } = self;
        ShapeRecord::Text {
            local_origin,
            text: store.record_text(text),
            color: color.into(),
            font,
            wrap,
            align,
        }
    }
}
