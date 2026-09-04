//! The text-run builder. Lowers to `ShapeRecord::Text`, with its source
//! normalized into the active text arena.

use crate::layout::types::align::Align;
use crate::primitives::color::RgbaF32;
use crate::primitives::interned_str::InternedStr;
use crate::primitives::nan::NanCheck;
use crate::scene::record_store::RecordStore;
use crate::scene::shapes::record::ShapeRecord;
use crate::shape::sealed;
use crate::text::font_family::FontFamily;
use crate::text::font_style::FontStyle;
use crate::text::font_weight::FontWeight;
use crate::text::glyph_font::GlyphFont;
use crate::text::wrap::TextWrap;
use glam::Vec2;

/// Shaped text run owned by the active node.
#[derive(Clone, Debug)]
pub struct TextShape {
    /// `None` → encoder owns positioning: the glyph bbox is placed
    /// inside the owner's padded inner rect via `align`. Used by
    /// Text/Button/ContextMenu.
    /// `Some(origin)` → widget owns positioning: bbox origin is
    /// `owner.min + origin`, encoder is a passthrough (`align`'s
    /// placement axes are ignored). Used by TextEdit so it can shift
    /// the text by scroll + alignment offsets the encoder can't
    /// compute.
    pub(crate) local_origin: Option<Vec2>,
    pub(crate) text: InternedStr,
    pub(crate) color: RgbaF32,
    /// The face and metrics to shape in — one named type rather than four
    /// fields, so this and [`ShapeRecord::Text`] mirror each other in one
    /// field and a shape cache key is minted from it directly.
    pub(crate) font: GlyphFont,
    pub(crate) wrap: TextWrap,
    /// Visual placement *and* cache-key discriminator: the encoder
    /// positions the glyph bbox inside the owner rect via both axes
    /// (only when `local_origin = None`), and the layout pipeline
    /// always threads `align.halign()` into cosmic's per-line
    /// `set_align` and text cache key. Same field because both
    /// consumers want the user-intended alignment.
    pub(crate) align: Align,
}

impl TextShape {
    pub(super) fn new(text: InternedStr, font: GlyphFont) -> Self {
        Self {
            local_origin: None,
            text,
            color: RgbaF32::WHITE,
            font,
            wrap: TextWrap::SingleLine,
            align: Align::TOP_LEFT,
        }
    }

    /// Hand positioning to the caller: the glyph bbox origin becomes
    /// `owner.min + origin` and the encoder stops placing it, so
    /// `align`'s placement axes go unread. Used by TextEdit, which
    /// shifts the run by scroll offsets the encoder cannot compute.
    ///
    /// Not `at`, which every rect-shaped kind spells for a whole
    /// [`Rect`](crate::Rect): a run has a pen position rather than a box,
    /// and one word cannot mean both.
    pub fn at_origin(mut self, origin: Vec2) -> Self {
        self.local_origin = Some(origin);
        self
    }
}
shape_setters!(TextShape {
    color: RgbaF32 => color,
    wrap: TextWrap => wrap,
    align: Align => align,
    family: FontFamily => font.family,
    weight: FontWeight => font.weight,
    style: FontStyle => font.style,
});

impl sealed::LowerShape for TextShape {
    /// An unusable face shapes nothing, which is what the two public
    /// text queries answer too — `TextShapeRequest::unbounded` is the one
    /// screen, and `metrics_valid` is its predicate.
    fn is_noop(&self) -> bool {
        self.text.is_empty() || self.color.is_noop() || !self.font.metrics_valid()
    }

    /// `font` is not asked again: `metrics_valid` above rejects a
    /// non-finite metric already, and it is the stricter question.
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
