use crate::layout::types::align::Align;
use crate::primitives::color::Color;
use crate::primitives::interned_str::InternedStr;
use crate::primitives::nan::NanCheck;
use crate::scene::record_store::RecordStore;
use crate::scene::shapes::record::ShapeRecord;
use crate::shape::sealed;
use crate::text::key;
use crate::text::wrap::TextWrap;
use crate::text::{FontFamily, FontWeight};
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
    pub(crate) color: Color,
    pub(crate) font_size_px: f32,
    pub(crate) line_height_px: f32,
    pub(crate) wrap: TextWrap,
    /// Visual placement *and* cache-key discriminator: the encoder
    /// positions the glyph bbox inside the owner rect via both axes
    /// (only when `local_origin = None`), and the layout pipeline
    /// always threads `align.halign()` into cosmic's per-line
    /// `set_align` and text cache key. Same field because both
    /// consumers want the user-intended alignment.
    pub(crate) align: Align,
    pub(crate) family: FontFamily,
    pub(crate) weight: FontWeight,
}

impl TextShape {
    pub(super) fn new(text: InternedStr, font_size_px: f32, line_height_px: f32) -> Self {
        Self {
            local_origin: None,
            text,
            color: Color::WHITE,
            font_size_px,
            line_height_px,
            wrap: TextWrap::SingleLine,
            align: Align::TOP_LEFT,
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
        }
    }

    pub fn color(mut self, color: impl Into<Color>) -> Self {
        self.color = color.into();
        self
    }

    pub fn wrap(mut self, wrap: impl Into<TextWrap>) -> Self {
        self.wrap = wrap.into();
        self
    }

    pub fn align(mut self, align: impl Into<Align>) -> Self {
        self.align = align.into();
        self
    }

    pub fn family(mut self, family: impl Into<FontFamily>) -> Self {
        self.family = family.into();
        self
    }

    pub fn weight(mut self, weight: impl Into<FontWeight>) -> Self {
        self.weight = weight.into();
        self
    }

    /// Hand positioning to the caller: the glyph bbox origin becomes
    /// `owner.min + origin` and the encoder stops placing it, so
    /// `align`'s placement axes go unread. Used by TextEdit, which
    /// shifts the run by scroll offsets the encoder cannot compute.
    pub fn at(mut self, origin: Vec2) -> Self {
        self.local_origin = Some(origin);
        self
    }
}
// See the `sealed` module in `shape/mod.rs` for why.
#[allow(private_interfaces)]
impl sealed::Lower for TextShape {
    fn is_noop(&self) -> bool {
        self.text.is_empty()
            || self.color.is_noop()
            // `text_metrics_valid` rejects NaN via `is_finite`;
            // `local_origin` needs saying. Worth catching here rather
            // than at the record gate: lowering interns the string into
            // the text arena, so a shape dropped afterwards would have
            // paid for that and left the bytes behind.
            || self.local_origin.has_nan()
            || !key::text_metrics_valid(self.font_size_px, self.line_height_px)
    }

    fn lower(self, store: &RecordStore) -> ShapeRecord {
        ShapeRecord::Text {
            local_origin: self.local_origin,
            text: store.record_text(self.text),
            color: self.color.into(),
            font_size_px: self.font_size_px,
            line_height_px: self.line_height_px,
            wrap: self.wrap,
            align: self.align,
            family: self.family,
            weight: self.weight,
        }
    }
}
