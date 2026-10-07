//! Canonical shaped-run identity: shaping parameters quantized into a
//! stable, purely integral cache key.

use crate::common::hash;
use crate::primitives::layout::align::HAlign;
use crate::primitives::math::domain::EPS;
use crate::primitives::math::num::F32Px;
use crate::text::RENDERED_RUN_KEEP_SPREAD_MASK;
use crate::text::font_family::FontFamily;
use crate::text::font_slant::FontSlant;
use crate::text::font_weight::FontWeight;
use crate::text::glyph_font::GlyphFont;
use crate::text::wrap::LineFit;
use std::num::NonZeroU64;

/// The face a [`TextShapeKey`] is measured at: size, family, weight and slant, without text or width.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct QuantizedFace {
    size_q: u32,
    family_q: u16,
    /// Weight and slant only; the bound half is masked off.
    face_q: u16,
}

/// Canonical shaping parameters and stable shaped-buffer identity.
///
/// Lossless, not a digest: the restore path rebuilds cosmic's `Metrics` and `Attrs` from the key alone.
///
/// Absence is `Option<Self>`; the non-zero `text_hash` keeps that free.
#[derive(Clone, Copy, Hash, Eq, PartialEq, Debug)]
pub(crate) struct TextShapeKey {
    /// Source hash, kept off zero by [`Self::content_hash`] so `Option<TextShapeKey>` stays 24 bytes.
    pub(crate) text_hash: NonZeroU64,
    /// `font_size * 64`, rounded.
    size_q: u32,
    /// `max_width * 64`, rounded; `u32::MAX` is unbounded. Always a multiple of 64: [`WrapBound::new`] snaps to whole px first.
    max_w_q: u32,
    /// `line_height * 64`, rounded.
    lh_q: u32,
    /// [`FontFamily`] index; meaningless outside the interning process, so keys are never persisted.
    family_q: u16,
    /// Weight, slant, line align and line fit; see [`FaceBits`].
    face_q: FaceBits,
}

const MAX_W_NONE: u32 = u32::MAX;

impl TextShapeKey {
    /// Share of the shaped-buffer cache's retention spread, see [`RENDERED_RUN_KEEP_SPREAD_MASK`].
    ///
    /// Mixes width and size in: identical labels at different widths are one text but many keys. `max_w_q`'s low six bits are always zero, so it is shifted down.
    pub(crate) const fn keep_spread(self) -> u64 {
        (self.text_hash.get() ^ (self.max_w_q as u64 >> 6) ^ self.size_q as u64)
            & RENDERED_RUN_KEEP_SPREAD_MASK
    }

    /// Content hash for a raw source hash; raw zero maps to one to fit [`NonZeroU64`].
    ///
    /// One definition because four sites derive it and must agree.
    pub(crate) const fn content_hash(raw: u64) -> NonZeroU64 {
        // `unwrap_or` is not const on `Option<NonZeroU64>`.
        match NonZeroU64::new(raw) {
            Some(hash) => hash,
            None => NonZeroU64::MIN,
        }
    }

    /// The unbounded key for `text` at `font`, or `None` where the face has no valid metrics.
    ///
    /// Unlike [`TextShapeRequest::unbounded`](crate::text::request::TextShapeRequest::unbounded), keeps empty text.
    pub(crate) fn for_text(text: &str, font: GlyphFont) -> Option<Self> {
        font.metrics_valid()
            .then(|| Self::unbounded(hash::hash_str(text), font))
    }

    /// The key `text_hash` shapes under at `font`, before any width.
    ///
    /// Callers screen the face first, so an invalid one is a logic error (debug-asserted); unscreened, a NaN size would quantize silently.
    pub(crate) fn unbounded(text_hash: u64, font: GlyphFont) -> Self {
        let GlyphFont {
            size: font_size,
            line_height,
            family,
            weight,
            slant,
        } = font;
        debug_assert!(
            GlyphFont::metrics_are_valid(font_size, line_height),
            "{}",
            GlyphFont::METRICS_ERROR,
        );
        Self {
            text_hash: Self::content_hash(text_hash),
            size_q: quantize_metric(font_size),
            max_w_q: MAX_W_NONE,
            lh_q: quantize_metric(line_height),
            family_q: family.raw(),
            face_q: FaceBits::new(weight, slant, LineAlign::Auto, LineFit::Wrap),
        }
    }

    /// Binds this root to a committed width; also rebuilds a bounded key from a retained [`WrapBound`].
    pub(super) const fn with_bound(self, bound: WrapBound) -> Self {
        Self {
            max_w_q: bound.max_w_q,
            face_q: self.face_q.with_bound(bound.bound_q),
            ..self
        }
    }

    pub(super) const fn unbounded_version(self) -> Self {
        Self {
            max_w_q: MAX_W_NONE,
            face_q: self
                .face_q
                .with_bound(FaceBits::bound_bits(LineAlign::Auto, LineFit::Wrap)),
            ..self
        }
    }

    /// The face without text or width. Narrow, so an ellipsis memo survives width changes.
    pub(super) const fn face(self) -> QuantizedFace {
        QuantizedFace {
            size_q: self.size_q,
            family_q: self.family_q,
            face_q: self.face_q.face_only(),
        }
    }

    pub(super) const fn font_size(self) -> f32 {
        dequantize(self.size_q)
    }

    pub(super) const fn line_height(self) -> f32 {
        dequantize(self.lh_q)
    }

    /// `line_height` snapped to the key's 1/64 px grid; values no key accepts come back unchanged.
    pub(crate) fn leading_on_grid(line_height: f32) -> f32 {
        if line_height.is_finite() && line_height > EPS {
            dequantize(quantize_metric(line_height))
        } else {
            line_height
        }
    }

    pub(crate) fn max_width(self) -> Option<f32> {
        (self.max_w_q != MAX_W_NONE).then(|| dequantize(self.max_w_q))
    }

    /// The family this key shapes in.
    pub(super) const fn family(self) -> FontFamily {
        FontFamily::from_raw(self.family_q)
    }

    pub(super) const fn weight(self) -> FontWeight {
        self.face_q.weight()
    }

    pub(super) const fn slant(self) -> FontSlant {
        self.face_q.slant()
    }

    /// `pub(crate)`: the text-edit suite asserts on it.
    pub(crate) const fn line_align(self) -> LineAlign {
        self.face_q.line_align()
    }

    pub(super) const fn fit(self) -> LineFit {
        self.face_q.fit()
    }
}

/// Weight, slant, line align and line fit in one 16-bit field.
///
/// Packing keeps the key at 24 bytes given the 10-bit weight. The bound half (align, fit) sits in bits 11..15, contiguous, as [`WrapBound`] rewrites exactly those; bit 15 is spare.
#[derive(Clone, Copy, Hash, Eq, PartialEq, Debug)]
pub(crate) struct FaceBits(u16);

const WEIGHT_MASK: u16 = (1 << 10) - 1;
const SLANT_SHIFT: u32 = 10;
const STYLE_MASK: u16 = 1 << SLANT_SHIFT;
const ALIGN_SHIFT: u32 = 11;
const ALIGN_MASK: u16 = 0b11 << ALIGN_SHIFT;
const FIT_SHIFT: u32 = 13;
const FIT_MASK: u16 = 0b11 << FIT_SHIFT;
const BOUND_MASK: u16 = ALIGN_MASK | FIT_MASK;

impl FaceBits {
    /// No range check: [`FontWeight`] is `1..=1000`, pinned inside [`WEIGHT_MASK`] below.
    const fn new(weight: FontWeight, slant: FontSlant, align: LineAlign, fit: LineFit) -> Self {
        Self(weight.get() | ((slant as u16) << SLANT_SHIFT) | Self::bound_bits(align, fit))
    }

    /// Align and fit as bits; the one place their positions are spelled.
    const fn bound_bits(align: LineAlign, fit: LineFit) -> u16 {
        ((align as u16) << ALIGN_SHIFT) | ((fit as u16) << FIT_SHIFT)
    }

    const fn with_bound(self, bound: u16) -> Self {
        debug_assert!(bound & !BOUND_MASK == 0);
        Self((self.0 & !BOUND_MASK) | bound)
    }

    /// Weight and slant alone, what [`QuantizedFace`] compares.
    const fn face_only(self) -> u16 {
        self.0 & !BOUND_MASK
    }

    /// The decoders make the last arm total so release has no panic path; the bits were written by this crate, so a bad tag is a logic error.
    const fn weight(self) -> FontWeight {
        FontWeight::from_raw(self.0 & WEIGHT_MASK)
    }

    const fn slant(self) -> FontSlant {
        match self.0 & STYLE_MASK {
            0 => FontSlant::Normal,
            _ => FontSlant::Italic,
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

/// The per-line alignment a key stores. [`HAlign::Stretch`] folds into `Auto`, as cosmic treats them alike; keeping them apart would reshape identical buffers.
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

/// Pins the discriminants the tag decoders resolve positionally, and the field widths.
///
/// A renumbered variant would silently decode to the wrong one in release.
const _: () = {
    assert!(FontSlant::Normal as u8 == 0 && FontSlant::Italic as u8 == 1);
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

/// What a committed width varies on a [`TextShapeKey`]; what [`TextShapeKey::with_bound`] writes.
///
/// Owns width normalization: canonicalized to the whole-px grid, negatives clamp to zero. Non-finite widths must be screened first. Separate from the key so reuse rows keep eight bytes, not 24.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct WrapBound {
    max_w_q: u32,
    /// Align and fit pre-packed as [`FaceBits`] holds them.
    bound_q: u16,
}

impl WrapBound {
    pub(super) fn new(max_width: f32, halign: HAlign, fit: LineFit) -> Self {
        debug_assert!(max_width.is_finite(), "text wrap width must be finite");
        let align = match fit {
            // A truncating fit is one line; nothing for align to move.
            LineFit::Wrap => LineAlign::from(halign),
            LineFit::Clip | LineFit::Ellipsis => LineAlign::Auto,
        };
        Self {
            max_w_q: quantize(max_width.canonical_px()).min(MAX_W_NONE - 1),
            bound_q: FaceBits::bound_bits(align, fit),
        }
    }
}

/// Length onto the 1/64-px grid; inverse of [`dequantize`].
#[expect(
    clippy::cast_sign_loss,
    reason = "the value is held at zero or above before the cast"
)]
fn quantize(value: f32) -> u32 {
    (value.max(0.0) * 64.0).fast_round() as u32
}

/// [`quantize`] floored at 1: a zero size or leading shapes nothing.
fn quantize_metric(value: f32) -> u32 {
    quantize(value).max(1)
}

const fn dequantize(value: u32) -> f32 {
    value as f32 / 64.0
}

// Also built by the `text_atlas` benchmark.
#[cfg(any(test, feature = "bench"))]
pub(crate) mod internals {
    use crate::text::glyph_font::GlyphFont;
    use crate::text::key::TextShapeKey;

    impl TextShapeKey {
        /// A key for a run nothing resolves, for fixtures that only need an identity.
        pub(crate) fn fixture() -> Self {
            Self::unbounded(1, GlyphFont::new(16.0))
        }
    }
}
