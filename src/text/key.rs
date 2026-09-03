//! Canonical shaped-run identity: shaping parameters quantized into a
//! stable, purely integral cache key.

use crate::common::hash;
use crate::layout::types::align::HAlign;
use crate::primitives::num::F32Ext;
use crate::text::RENDERED_RUN_KEEP_SPREAD_MASK;
use crate::text::font_family::FontFamily;
use crate::text::font_style::FontStyle;
use crate::text::font_weight::FontWeight;
use crate::text::glyph_font::GlyphFont;
use crate::text::wrap::{self, LineFit};

/// The face a shape is measured at, as [`TextShapeKey`] quantizes it:
/// size, family, weight and style, and nothing about the text or the
/// width.
///
/// Named rather than compared field by field wherever a face has to
/// match — the equality is derived, so a fourth field cannot be added to
/// the key's face and left out of one comparison.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct QuantizedFace {
    size_q: u32,
    family_q: u16,
    /// [`FaceBits`] with the bound half masked off — the weight and style
    /// alone, since a face is the same face however wide its box is.
    face_q: u16,
}

/// Canonical shaping parameters and stable shaped-buffer identity. Layout
/// derives it from `ShapeRecord::Text`; the encoder carries it through the
/// composer so the renderer can restore the matching buffer without rehashing
/// or reconstructing a second parameter representation.
///
/// Three quantized fields rather than one collapsed `u64` so the renderer
/// can also reuse the size/width components if it wants to (e.g. group runs
/// by size for atlas bin reuse). [`TextShapeKey::INVALID`] tags a measurement
/// with no shaped buffer — the encoder drops those runs before paint.
#[derive(Clone, Copy, Hash, Eq, PartialEq, Debug)]
pub(crate) struct TextShapeKey {
    /// 64-bit hash of the source string. `0` for the invalid sentinel.
    pub(crate) text_hash: u64,
    /// `font_size_px * 64`, rounded. Quantizing to 1/64 px is below any
    /// visible difference and keeps the key purely integral.
    pub(crate) size_q: u32,
    /// `max_width_px * 64`, rounded; `u32::MAX` encodes `None` (unbounded).
    ///
    /// Unlike its 1/64-px neighbours this carries only whole pixels:
    /// [`WrapBound::new`] snaps the width to the measure cache's grid via
    /// [`wrap::canonical_wrap_width`] first, so the value is always a
    /// multiple of 64. The scale is kept so all three quantized fields
    /// dequantize the same way, not because the precision is reachable.
    pub(crate) max_w_q: u32,
    /// `line_height_px * 64`, rounded. Two `ShapeRecord::Text` runs at the
    /// same font-size but different leading produce different shaped
    /// buffers (different `Metrics::new`), so the key has to discriminate.
    pub(crate) lh_q: u32,
    /// [`FontFamily`] index. Two runs with identical text/size but
    /// different families produce different shaped buffers, so the key
    /// has to discriminate. The index means nothing outside the process
    /// that interned it, which is why no key is ever persisted.
    pub(crate) family_q: u16,
    /// Weight, style, per-line alignment and line fit — see
    /// [`FaceBits`], which is where the four stop being separate bytes
    /// so the key can hold a 10-bit weight and stay 24 bytes.
    pub(crate) face_q: FaceBits,
}

const MAX_W_NONE: u32 = u32::MAX;

impl TextShapeKey {
    /// Sentinel for a measurement with no shaped buffer: in production,
    /// empty text and a face with no usable size — the two runs
    /// [`TextShapeRequest`](crate::text::request::TextShapeRequest) mints
    /// nothing for — plus every run under the test-only mono fallback.
    /// Real keys always carry a nonzero text hash, so that field alone
    /// tags validity.
    pub(crate) const INVALID: Self = Self {
        text_hash: 0,
        size_q: 0,
        max_w_q: 0,
        lh_q: 0,
        family_q: 0,
        // The stock face rather than a zeroed one: the four accessors
        // decode without a validity check, and a zero weight is a value
        // [`FontWeight`] refuses to name.
        face_q: FaceBits::STOCK,
    };

    pub(crate) const fn is_invalid(self) -> bool {
        self.text_hash == 0
    }

    /// This key's share of the shaped-buffer cache's retention spread —
    /// see [`RENDERED_RUN_KEEP_SPREAD_MASK`].
    ///
    /// Mixes width and size into the text hash rather than taking that
    /// hash alone: a list of identically-labelled rows at different
    /// widths is one text and many keys, and it is the keys that expire.
    /// `max_w_q` counts 1/64ths of a width [`WrapBound::new`] already
    /// snapped to whole pixels, so its low six bits are always zero and
    /// it contributes nothing until they are shifted off.
    pub(crate) const fn keep_spread(self) -> u64 {
        (self.text_hash ^ (self.max_w_q as u64 >> 6) ^ self.size_q as u64)
            & RENDERED_RUN_KEEP_SPREAD_MASK
    }

    /// The content hash a key carries, given the raw hash of its source:
    /// never zero, because zero is what [`Self::INVALID`] tags a
    /// bufferless run with.
    ///
    /// Stated once because four sites derive it — this key's constructor,
    /// both of `ShapedTextRef`'s pairing checks, and `TextEdit`'s
    /// history reconciliation, which compares a hash it mints itself
    /// against one that came off a key. A rule spelled at each of them is
    /// one that can be changed in only some, leaving the checks agreeing
    /// with a hash nothing mints any more.
    pub(crate) const fn content_hash(raw: u64) -> u64 {
        // `Ord::max` is not const yet, and the zero case is the whole rule.
        if raw == 0 { 1 } else { raw }
    }

    /// The face is screened before this is reached — by
    /// [`TextShapeRequest::unbounded`](crate::text::request::TextShapeRequest::unbounded)
    /// on every shaping path and by `TextRun::unbounded_key` on the
    /// bufferless one, both through
    /// [`GlyphFont::metrics_are_valid`] — so an unusable one here is a
    /// logic error, debug-asserted rather than re-validated on the
    /// shaping hot path. Without that screen the quantization below is
    /// silent: a NaN size lands on a 1/64-px face and shapes against it.
    /// The unbounded key for `text` at `font`, or [`Self::INVALID`]
    /// where the face has no metrics to answer in.
    ///
    /// **The one minting path.**
    /// [`TextShapeRequest::unbounded`](crate::text::request::TextShapeRequest::unbounded)
    /// refuses empty text on top of this, because there is nothing to
    /// shape; a run's own key keeps it, because the metrics a probe
    /// answers in live on the key whether or not a buffer does.
    pub(crate) fn for_text(text: &str, font: GlyphFont) -> Self {
        if !font.metrics_valid() {
            return Self::INVALID;
        }
        Self::unbounded(hash::hash_str(text), font)
    }

    pub(crate) fn unbounded(text_hash: u64, font: GlyphFont) -> Self {
        let GlyphFont {
            size_px: font_size_px,
            line_height_px,
            family,
            weight,
            style,
        } = font;
        debug_assert!(
            GlyphFont::metrics_are_valid(font_size_px, line_height_px),
            "{}",
            GlyphFont::METRICS_ERROR,
        );
        Self {
            text_hash: Self::content_hash(text_hash),
            size_q: quantize_metric(font_size_px),
            max_w_q: MAX_W_NONE,
            lh_q: quantize_metric(line_height_px),
            family_q: family.raw(),
            face_q: FaceBits::new(weight, style, HAlign::Auto, LineFit::Wrap),
        }
    }

    /// Bind this root to a committed width. The bound varies nothing a root
    /// key pins, so this both *mints* a bounded key and *reconstructs* one
    /// from a retained [`WrapBound`] — which is what lets `TextSystem` keep
    /// six bytes per resolve rather than a second whole key.
    ///
    /// Taking the bound rather than `(width, halign, fit)` is what keeps a
    /// caller that needs the bound for something else — a slot comparison,
    /// say — from quantizing the same width twice.
    pub(super) fn with_bound(self, bound: WrapBound) -> Self {
        Self {
            max_w_q: bound.max_w_q,
            face_q: self.face_q.with_bound(bound.bound_q),
            ..self
        }
    }

    pub(super) fn unbounded_version(self) -> Self {
        Self {
            max_w_q: MAX_W_NONE,
            face_q: self
                .face_q
                .with_bound(FaceBits::bound_bits(HAlign::Auto, LineFit::Wrap)),
            ..self
        }
    }

    /// The face this key shapes at, without its text or its width — the
    /// three fields [`Self::unbounded`] folded a [`GlyphFont`] into.
    ///
    /// Quantized, so it compares the way the key does, and *narrow*: an
    /// ellipsis is the same glyph at the same face however wide the box
    /// is, which is what lets its memo survive the width churn it exists
    /// for.
    pub(super) fn face(self) -> QuantizedFace {
        QuantizedFace {
            size_q: self.size_q,
            family_q: self.family_q,
            face_q: self.face_q.face_only(),
        }
    }

    pub(super) fn font_size_px(self) -> f32 {
        dequantize(self.size_q)
    }

    pub(super) fn line_height_px(self) -> f32 {
        dequantize(self.lh_q)
    }

    pub(super) fn max_width_px(self) -> Option<f32> {
        (self.max_w_q != MAX_W_NONE).then(|| dequantize(self.max_w_q))
    }

    /// The family this key shapes in. Any index the table has handed out
    /// is valid, and `CosmicMeasure` is what decides whether a face
    /// answers to it — see `font_available`.
    pub(super) fn family(self) -> FontFamily {
        FontFamily::from_raw(self.family_q)
    }

    pub(super) fn weight(self) -> FontWeight {
        self.face_q.weight()
    }

    pub(super) fn style(self) -> FontStyle {
        self.face_q.style()
    }

    /// `pub(crate)` where its siblings are `pub(super)`: the text-edit
    /// suite asserts on the alignment a rendered buffer was keyed under,
    /// and it lives outside `crate::text`.
    pub(crate) fn halign(self) -> HAlign {
        self.face_q.halign()
    }

    pub(super) fn fit(self) -> LineFit {
        self.face_q.fit()
    }
}

/// Weight, style, per-line alignment and line fit in one 16-bit field.
///
/// Four separate bytes is what the key used to hold, and it fitted only
/// while weight was a two-variant enum. A weight is a number on the CSS
/// 1–1000 scale, which wants ten bits, and style is a fifth axis — as
/// bytes the two would push the key from 24 to 32 and widen every
/// `ShapeRecord::Text` beside it. Packed, the whole face costs the two
/// bytes the family does not use.
///
/// The bound half — [`HAlign`] and [`LineFit`] — sits in the top five
/// bits, contiguous, because [`WrapBound`] rewrites exactly those and
/// nothing else.
#[derive(Clone, Copy, Hash, Eq, PartialEq, Debug)]
pub(crate) struct FaceBits(u16);

const WEIGHT_MASK: u16 = (1 << 10) - 1;
const STYLE_SHIFT: u32 = 10;
const STYLE_MASK: u16 = 1 << STYLE_SHIFT;
const HALIGN_SHIFT: u32 = 11;
const HALIGN_MASK: u16 = 0b111 << HALIGN_SHIFT;
const FIT_SHIFT: u32 = 14;
const FIT_MASK: u16 = 0b11 << FIT_SHIFT;
const BOUND_MASK: u16 = HALIGN_MASK | FIT_MASK;

impl FaceBits {
    /// What [`TextShapeKey::INVALID`] carries: the default face, so a
    /// sentinel decodes to values every axis calls its own.
    const STOCK: Self = Self::new(
        FontWeight::REGULAR,
        FontStyle::Normal,
        HAlign::Auto,
        LineFit::Wrap,
    );

    /// No range check on the weight: [`FontWeight`] holds nothing outside
    /// `1..=1000`, and the `const _` block below pins that inside
    /// [`WEIGHT_MASK`].
    const fn new(weight: FontWeight, style: FontStyle, halign: HAlign, fit: LineFit) -> Self {
        Self(weight.value() | ((style as u16) << STYLE_SHIFT) | Self::bound_bits(halign, fit))
    }

    /// The two fields a committed width rewrites, as bits — the one place
    /// their positions are spelled, so [`WrapBound`] can carry them
    /// without carrying the whole face.
    const fn bound_bits(halign: HAlign, fit: LineFit) -> u16 {
        ((halign as u16) << HALIGN_SHIFT) | ((fit as u16) << FIT_SHIFT)
    }

    const fn with_bound(self, bound: u16) -> Self {
        debug_assert!(bound & !BOUND_MASK == 0);
        Self((self.0 & !BOUND_MASK) | bound)
    }

    /// The weight and style alone — the half of a face that survives a
    /// width change, which is what [`QuantizedFace`] compares.
    const fn face_only(self) -> u16 {
        self.0 & !BOUND_MASK
    }

    /// The three variant decoders below make their last arm total, so
    /// release builds decode with a jump table and no panic path.
    ///
    /// Every one of these bit fields was written by this crate from the
    /// axis itself, so a bad tag is a logic error here, never bad data —
    /// and these run per shape, which release builds must not pay a check
    /// for. The restore path is what forces the round-trip to exist at
    /// all; see `CosmicMeasure::shape_truncated`.
    const fn weight(self) -> FontWeight {
        FontWeight::from_raw(self.0 & WEIGHT_MASK)
    }

    const fn style(self) -> FontStyle {
        match self.0 & STYLE_MASK {
            0 => FontStyle::Normal,
            _ => FontStyle::Italic,
        }
    }

    const fn halign(self) -> HAlign {
        match (self.0 & HALIGN_MASK) >> HALIGN_SHIFT {
            0 => HAlign::Auto,
            1 => HAlign::Left,
            2 => HAlign::Center,
            3 => HAlign::Right,
            _ => HAlign::Stretch,
        }
    }

    const fn fit(self) -> LineFit {
        match (self.0 & FIT_MASK) >> FIT_SHIFT {
            0 => LineFit::Wrap,
            1 => LineFit::Clip,
            _ => LineFit::Ellipsis,
        }
    }
}

/// Every discriminant the tag decoders resolve positionally, and the
/// widths the packing assumes.
///
/// The decoders map tag `n` to the `n`th variant and let the last arm
/// swallow everything above it, so a renumbered or reordered variant would
/// resolve every cached key to the *wrong* one — in release, silently,
/// with the value still inside the field it was read from. Naming each
/// discriminant here turns that into a build failure beside the code that
/// depends on it. The two width assertions are the other half: a fifth
/// `LineFit` or a sixth `HAlign` would silently overflow its field.
const _: () = {
    assert!(FontStyle::Normal as u8 == 0 && FontStyle::Italic as u8 == 1);
    assert!(
        HAlign::Auto as u8 == 0
            && HAlign::Left as u8 == 1
            && HAlign::Center as u8 == 2
            && HAlign::Right as u8 == 3
            && HAlign::Stretch as u8 == 4
    );
    assert!(LineFit::Wrap as u8 == 0 && LineFit::Clip as u8 == 1 && LineFit::Ellipsis as u8 == 2);
    assert!(HAlign::Stretch as u16 <= (HALIGN_MASK >> HALIGN_SHIFT));
    assert!(LineFit::Ellipsis as u16 <= (FIT_MASK >> FIT_SHIFT));
    assert!(FontWeight::MAX <= WEIGHT_MASK);
};

/// Everything a committed width varies on a [`TextShapeKey`]: the three
/// fields [`TextShapeKey::with_bound`] writes, and nothing else.
///
/// Owns the width normalization — the raw width canonicalizes to the
/// whole-px wrap grid here. Negative widths (over-constrained layouts)
/// clamp to zero. A non-finite width names no grid to wrap to, and
/// quantizes to the largest bound the key can hold rather than to
/// anything a caller meant, so it is screened before this: layout binds
/// a width it derived from an arranged rect, and a probe binds
/// `TextRun::wrap_width`, which drops one.
///
/// Separate from the key because `TextSystem`'s reuse rows retain one per
/// bounded resolve, and eight bytes beside a root key they already hold
/// beats a second 24-byte key. Computing it *here* rather than by
/// quantizing through a throwaway key is what keeps the sentinel meaning
/// one thing: [`TextShapeKey::INVALID`] names a run with no shaped buffer,
/// never a scratch value to hang a width off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct WrapBound {
    max_w_q: u32,
    /// The align and fit already packed where [`FaceBits`] holds them, so
    /// binding a width is a mask and an or rather than a second encode.
    bound_q: u16,
}

impl WrapBound {
    pub(super) fn new(max_width_px: f32, halign: HAlign, fit: LineFit) -> Self {
        debug_assert!(max_width_px.is_finite(), "text wrap width must be finite");
        let halign = match fit {
            // Projected onto what shaping actually varies on, not
            // stored raw: `cosmic_align` maps `Auto` and `Stretch`
            // alike to "no per-line align", so keeping them apart
            // here minted two keys, two cache entries and two
            // reshapes for a byte-identical buffer. `Stretch` is a
            // box-alignment concept with no per-line meaning.
            LineFit::Wrap => match halign {
                HAlign::Auto | HAlign::Stretch => HAlign::Auto,
                other => other,
            },
            LineFit::Clip | LineFit::Ellipsis => HAlign::Auto,
        };
        Self {
            max_w_q: quantize(wrap::canonical_wrap_width(max_width_px)).min(MAX_W_NONE - 1),
            bound_q: FaceBits::bound_bits(halign, fit),
        }
    }
}

/// A length onto the 1/64-px grid every key field is stored on, the
/// inverse of [`dequantize`]. Any length — a width, a size, a leading —
/// since what varies between them is the floor, not the grid.
fn quantize(value: f32) -> u32 {
    (value.max(0.0) * 64.0).fast_round() as u32
}

/// [`quantize`] floored at one 1/64th: a font size or a line height of
/// zero would shape nothing, so the key never carries one.
fn quantize_metric(value: f32) -> u32 {
    quantize(value).max(1)
}

fn dequantize(value: u32) -> f32 {
    value as f32 / 64.0
}
