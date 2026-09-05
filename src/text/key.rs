//! Canonical shaped-run identity: shaping parameters quantized into a
//! stable, purely integral cache key.

use crate::common::hash;
use crate::layout::types::align::HAlign;
use crate::primitives::num::F32Px;
use crate::text::RENDERED_RUN_KEEP_SPREAD_MASK;
use crate::text::font_family::FontFamily;
use crate::text::font_style::FontStyle;
use crate::text::font_weight::FontWeight;
use crate::text::glyph_font::GlyphFont;
use crate::text::wrap::{self, LineFit};
use std::num::NonZeroU64;

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
/// **Lossless, not a digest.** Every shaping input is stored quantized
/// rather than folded into the text's hash, because the restore path
/// rebuilds cosmic's `Metrics` and `Attrs` from the key alone: a run
/// whose buffer aged out reshapes from what the key says, and by then
/// nothing else survives to say it.
///
/// **A run that names no key says so in an `Option`.** Every producer of
/// one hands back `Option<Self>`, and [`Self::content_hash`] keeps the
/// hash off zero so that option costs no width — see the field.
#[derive(Clone, Copy, Hash, Eq, PartialEq, Debug)]
pub(crate) struct TextShapeKey {
    /// 64-bit hash of the source string, mapped off zero by
    /// [`Self::content_hash`].
    ///
    /// Non-zero as a *type*, so the zero bit pattern is free for
    /// `Option<TextShapeKey>`'s niche: a run with no shapeable face is
    /// spelled `None` and still costs the 24 bytes a key does. That is
    /// what lets absence be a case the compiler makes every holder
    /// answer, rather than a value each of them has to remember to
    /// screen for.
    pub(crate) text_hash: NonZeroU64,
    /// `font_size_px * 64`, rounded. Quantizing to 1/64 px is below any
    /// visible difference and keeps the key purely integral.
    size_q: u32,
    /// `max_width_px * 64`, rounded; `u32::MAX` encodes `None` (unbounded).
    ///
    /// Unlike its 1/64-px neighbours this carries only whole pixels:
    /// [`WrapBound::new`] snaps the width to the measure cache's grid via
    /// [`wrap::canonical_wrap_width`] first, so the value is always a
    /// multiple of 64. The scale is kept so all three quantized fields
    /// dequantize the same way, not because the precision is reachable.
    max_w_q: u32,
    /// `line_height_px * 64`, rounded. Two `ShapeRecord::Text` runs at the
    /// same font-size but different leading produce different shaped
    /// buffers (different `Metrics::new`), so the key has to discriminate.
    lh_q: u32,
    /// [`FontFamily`] index. Two runs with identical text/size but
    /// different families produce different shaped buffers, so the key
    /// has to discriminate. The index means nothing outside the process
    /// that interned it, which is why no key is ever persisted.
    family_q: u16,
    /// Weight, style, per-line alignment and line fit — see
    /// [`FaceBits`], which is where the four stop being separate bytes
    /// so the key can hold a 10-bit weight and stay 24 bytes.
    face_q: FaceBits,
}

const MAX_W_NONE: u32 = u32::MAX;

impl TextShapeKey {
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
        (self.text_hash.get() ^ (self.max_w_q as u64 >> 6) ^ self.size_q as u64)
            & RENDERED_RUN_KEEP_SPREAD_MASK
    }

    /// The content hash a key carries, given the raw hash of its source.
    ///
    /// Raw zero maps to one so the field can be a [`NonZeroU64`], which
    /// is what makes `Option<TextShapeKey>` as wide as a key. The two
    /// strings whose raw hashes are zero and one therefore share an
    /// identity, which costs them one shaped buffer between them and
    /// nothing a reader can see.
    ///
    /// Stated once because four sites derive it — this key's constructor,
    /// both of `ShapedTextRef`'s pairing checks, and `TextEdit`'s
    /// history reconciliation, which compares a hash it mints itself
    /// against one that came off a key. A rule spelled at each of them is
    /// one that can be changed in only some, leaving the checks agreeing
    /// with a hash nothing mints any more.
    pub(crate) const fn content_hash(raw: u64) -> NonZeroU64 {
        // `unwrap_or` is not const on `Option<NonZeroU64>`, and the zero
        // case is the whole rule.
        match NonZeroU64::new(raw) {
            Some(hash) => hash,
            None => NonZeroU64::MIN,
        }
    }

    /// The unbounded key for `text` at `font`, or `None` where the face
    /// has no metrics to answer in.
    ///
    /// **The one screening path.**
    /// [`TextShapeRequest::unbounded`](crate::text::request::TextShapeRequest::unbounded)
    /// refuses empty text on top of this, because there is nothing to
    /// shape; a run's own key keeps it, because the metrics a probe
    /// answers in live on the key whether or not a buffer does.
    pub(crate) fn for_text(text: &str, font: GlyphFont) -> Option<Self> {
        font.metrics_valid()
            .then(|| Self::unbounded(hash::hash_str(text), font))
    }

    /// The key `text_hash` shapes under at `font`, before any width is
    /// bound to it.
    ///
    /// The face is screened before this is reached — by
    /// [`Self::for_text`] on the probe path and by `TextShape::is_noop`
    /// on the recorded one, both through
    /// [`GlyphFont::metrics_are_valid`] — so an unusable one here is a
    /// logic error, debug-asserted rather than re-validated on the
    /// shaping hot path. Without that screen the quantization below is
    /// silent: a NaN size lands on a 1/64-px face and shapes against it.
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
            face_q: FaceBits::new(weight, style, LineAlign::Auto, LineFit::Wrap),
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
                .with_bound(FaceBits::bound_bits(LineAlign::Auto, LineFit::Wrap)),
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

    pub(crate) fn max_width_px(self) -> Option<f32> {
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
    pub(crate) fn line_align(self) -> LineAlign {
        self.face_q.line_align()
    }

    pub(super) fn fit(self) -> LineFit {
        self.face_q.fit()
    }
}

/// Weight, style, per-line alignment and line fit in one 16-bit field.
///
/// A weight is a number on the CSS 1–1000 scale, which wants ten bits,
/// so the four axes as separate bytes would push the key from 24 to 32
/// and widen every `ShapeRecord::Text` beside it. Packed, the whole face
/// costs the two bytes the family does not use.
///
/// The bound half — [`LineAlign`] and [`LineFit`] — sits in bits 11..15,
/// contiguous, because [`WrapBound`] rewrites exactly those and nothing
/// else. Bit 15 is spare, because the align holds the four values
/// shaping varies on rather than the five [`HAlign`] names.
#[derive(Clone, Copy, Hash, Eq, PartialEq, Debug)]
pub(crate) struct FaceBits(u16);

const WEIGHT_MASK: u16 = (1 << 10) - 1;
const STYLE_SHIFT: u32 = 10;
const STYLE_MASK: u16 = 1 << STYLE_SHIFT;
const ALIGN_SHIFT: u32 = 11;
const ALIGN_MASK: u16 = 0b11 << ALIGN_SHIFT;
const FIT_SHIFT: u32 = 13;
const FIT_MASK: u16 = 0b11 << FIT_SHIFT;
const BOUND_MASK: u16 = ALIGN_MASK | FIT_MASK;

impl FaceBits {
    /// No range check on the weight: [`FontWeight`] holds nothing outside
    /// `1..=1000`, and the `const _` block below pins that inside
    /// [`WEIGHT_MASK`].
    const fn new(weight: FontWeight, style: FontStyle, align: LineAlign, fit: LineFit) -> Self {
        Self(weight.value() | ((style as u16) << STYLE_SHIFT) | Self::bound_bits(align, fit))
    }

    /// The two fields a committed width rewrites, as bits — the one place
    /// their positions are spelled, so [`WrapBound`] can carry them
    /// without carrying the whole face.
    const fn bound_bits(align: LineAlign, fit: LineFit) -> u16 {
        ((align as u16) << ALIGN_SHIFT) | ((fit as u16) << FIT_SHIFT)
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

    const fn line_align(self) -> LineAlign {
        match (self.0 & ALIGN_MASK) >> ALIGN_SHIFT {
            0 => LineAlign::Auto,
            1 => LineAlign::Left,
            2 => LineAlign::Center,
            _ => LineAlign::Right,
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

/// The per-line alignment a [`TextShapeKey`] stores: the four values
/// shaping varies on.
///
/// [`HAlign::Stretch`] is not among them, and the conversion is where it
/// goes. Cosmic maps `Stretch` and `Auto` alike to "no per-line align" —
/// `Stretch` is a box-alignment concept and a line has no box — so a key
/// that kept the two apart minted two entries and two reshapes for a
/// byte-identical buffer. Projecting into a type the encoder takes makes
/// that a conversion rather than a rule each writer has to remember.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LineAlign {
    Auto = 0,
    Left = 1,
    Center = 2,
    Right = 3,
}

impl From<HAlign> for LineAlign {
    fn from(halign: HAlign) -> Self {
        match halign {
            HAlign::Auto | HAlign::Stretch => Self::Auto,
            HAlign::Left => Self::Left,
            HAlign::Center => Self::Center,
            HAlign::Right => Self::Right,
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
/// `LineFit` or a fifth `LineAlign` would silently overflow its field.
const _: () = {
    assert!(FontStyle::Normal as u8 == 0 && FontStyle::Italic as u8 == 1);
    assert!(
        LineAlign::Auto as u8 == 0
            && LineAlign::Left as u8 == 1
            && LineAlign::Center as u8 == 2
            && LineAlign::Right as u8 == 3
    );
    assert!(LineFit::Wrap as u8 == 0 && LineFit::Clip as u8 == 1 && LineFit::Ellipsis as u8 == 2);
    assert!(LineAlign::Right as u16 <= (ALIGN_MASK >> ALIGN_SHIFT));
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
/// quantizing through a throwaway key is what keeps every minted
/// [`TextShapeKey`] a run the shaper can be asked for, never a scratch
/// value to hang a width off.
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
        let align = match fit {
            // A truncating fit is one line, and cosmic aligns per line,
            // so there is nothing for an align to move.
            LineFit::Wrap => LineAlign::from(halign),
            LineFit::Clip | LineFit::Ellipsis => LineAlign::Auto,
        };
        Self {
            max_w_q: quantize(wrap::canonical_wrap_width(max_width_px)).min(MAX_W_NONE - 1),
            bound_q: FaceBits::bound_bits(align, fit),
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

// Gated as wide as its consumers: the encoded cache's churn fixture is
// built by the `text_atlas` benchmark as well as by tests.
#[cfg(any(test, feature = "bench"))]
pub(crate) mod test_support {
    use crate::text::glyph_font::GlyphFont;
    use crate::text::key::TextShapeKey;

    impl TextShapeKey {
        /// A key standing for a run nothing resolves.
        ///
        /// Fixtures about batching, geometry, or the *other* components
        /// of a cache key built over this one need a run identity and
        /// nothing else. One shared spelling, so none of them implies
        /// its own face matters.
        pub(crate) fn fixture() -> Self {
            Self::unbounded(1, GlyphFont::new(16.0))
        }
    }
}
