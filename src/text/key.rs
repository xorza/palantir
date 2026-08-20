//! Canonical shaped-run identity: shaping parameters quantized into a
//! stable, purely integral cache key.

use crate::layout::types::align::HAlign;
use crate::primitives::num::F32Ext;
use crate::text::glyph_font::GlyphFont;
use crate::text::wrap::{self, LineFit};
use crate::text::{FontFamily, FontWeight};

/// Canonical shaping parameters and stable shaped-buffer identity. Layout
/// derives it from `ShapeRecord::Text`; the encoder carries it through the
/// composer so the renderer can restore the matching buffer without rehashing
/// or reconstructing a second parameter representation.
///
/// Three quantized fields rather than one collapsed `u64` so the renderer
/// can also reuse the size/width components if it wants to (e.g. group runs
/// by size for atlas bin reuse). [`TextShapeKey::INVALID`] tags a measurement
/// with no shaped buffer — the encoder drops those runs before paint.
#[repr(C)]
#[derive(Clone, Copy, Hash, Eq, PartialEq, Debug, bytemuck::Pod, bytemuck::Zeroable)]
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
    /// [`FontFamily`] discriminant. Two runs with identical text/size
    /// but different families produce different shaped buffers, so the
    /// key has to discriminate. `u8` because `FontFamily` is `#[repr(u8)]`.
    pub(crate) family_q: u8,
    /// [`FontWeight`] discriminant. Two runs with identical text/size/
    /// family but different weight shape against different physical faces
    /// (Regular vs Bold), so the key has to discriminate.
    pub(crate) weight_q: u8,
    /// [`HAlign`] discriminant for per-line text alignment. Cosmic
    /// shapes the buffer with line-internal x offsets that depend on
    /// the per-line align, so two runs with identical text/size but
    /// different halign produce different shaped buffers and the key
    /// has to discriminate. `0` (`HAlign::Auto`) means "no per-line
    /// alignment" and matches the previous behaviour.
    pub(crate) halign_q: u8,
    /// [`LineFit`] discriminant. Truncating fits bake different source text
    /// into the shaped buffer at the same width, so fit is independent cache
    /// identity rather than part of the text-content hash. This occupies the
    /// former trailing padding byte, keeping the key at 24 bytes.
    pub(crate) fit_q: u8,
}

const MAX_W_NONE: u32 = u32::MAX;

impl TextShapeKey {
    /// Sentinel for a measurement with no shaped buffer: empty text in
    /// production, plus every run under the test-only mono fallback. Real
    /// keys always carry a nonzero text hash, so that field alone tags
    /// validity.
    pub(crate) const INVALID: Self = Self {
        text_hash: 0,
        size_q: 0,
        max_w_q: 0,
        lh_q: 0,
        family_q: 0,
        weight_q: 0,
        halign_q: 0,
        fit_q: 0,
    };

    pub(crate) const fn is_invalid(self) -> bool {
        self.text_hash == 0
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

    /// Record time already rejected invalid metrics (`Shape::is_noop`,
    /// theme validation), so reaching here with them is a logic error —
    /// debug-asserted rather than re-validated on the shaping hot path.
    pub(crate) fn unbounded(text_hash: u64, font: GlyphFont) -> Self {
        let GlyphFont {
            size_px: font_size_px,
            line_height_px,
            family,
            weight,
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
            family_q: family as u8,
            weight_q: weight as u8,
            halign_q: HAlign::Auto as u8,
            fit_q: LineFit::Wrap as u8,
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
            halign_q: bound.halign_q,
            fit_q: bound.fit_q,
            ..self
        }
    }

    pub(super) fn unbounded_version(self) -> Self {
        Self {
            max_w_q: MAX_W_NONE,
            halign_q: HAlign::Auto as u8,
            fit_q: LineFit::Wrap as u8,
            ..self
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

    /// The four decoders below `debug_assert` their range and then make
    /// the last variant total, so release builds decode with a jump
    /// table and no panic path.
    ///
    /// Every one of these bytes was written by this crate from the enum
    /// itself (`family as u8`), so a bad tag is a logic error here, never
    /// bad data — and these run per shape, which release builds must not
    /// pay a check for. The restore path is what forces the round-trip to
    /// exist at all; see `CosmicMeasure::shape_truncated`.
    ///
    /// They stay four hand-written functions rather than one generic
    /// decoder: each has to name its own variants, so a macro or a trait
    /// would relocate that list rather than remove it. What *is* shared —
    /// the assumption that tag `n` means the `n`th variant — is pinned
    /// once by the `const _` assertion below this block instead.
    pub(super) fn family(self) -> FontFamily {
        debug_assert!(
            self.family_q <= FontFamily::Mono as u8,
            "invalid FontFamily tag {}",
            self.family_q
        );
        match self.family_q {
            0 => FontFamily::Sans,
            _ => FontFamily::Mono,
        }
    }

    pub(super) fn weight(self) -> FontWeight {
        debug_assert!(
            self.weight_q <= FontWeight::Bold as u8,
            "invalid FontWeight tag {}",
            self.weight_q
        );
        match self.weight_q {
            0 => FontWeight::Regular,
            _ => FontWeight::Bold,
        }
    }

    pub(super) fn halign(self) -> HAlign {
        debug_assert!(
            self.halign_q <= HAlign::Stretch as u8,
            "invalid HAlign tag {}",
            self.halign_q
        );
        match self.halign_q {
            0 => HAlign::Auto,
            1 => HAlign::Left,
            2 => HAlign::Center,
            3 => HAlign::Right,
            _ => HAlign::Stretch,
        }
    }

    pub(super) fn fit(self) -> LineFit {
        debug_assert!(
            self.fit_q <= LineFit::Ellipsis as u8,
            "invalid LineFit tag {}",
            self.fit_q
        );
        match self.fit_q {
            0 => LineFit::Wrap,
            1 => LineFit::Clip,
            _ => LineFit::Ellipsis,
        }
    }
}

/// Every discriminant the tag decoders resolve positionally.
///
/// The decoders map tag `n` to the `n`th variant and let the last arm
/// swallow everything above it, so a renumbered or reordered variant would
/// resolve every cached key to the *wrong* one — in release, silently,
/// with the value still inside the range the `debug_assert`s check. Naming
/// each discriminant here turns that into a build failure beside the code
/// that depends on it.
const _: () = {
    assert!(FontFamily::Sans as u8 == 0 && FontFamily::Mono as u8 == 1);
    assert!(FontWeight::Regular as u8 == 0 && FontWeight::Bold as u8 == 1);
    assert!(
        HAlign::Auto as u8 == 0
            && HAlign::Left as u8 == 1
            && HAlign::Center as u8 == 2
            && HAlign::Right as u8 == 3
            && HAlign::Stretch as u8 == 4
    );
    assert!(LineFit::Wrap as u8 == 0 && LineFit::Clip as u8 == 1 && LineFit::Ellipsis as u8 == 2);
};

/// Everything a committed width varies on a [`TextShapeKey`]: the three
/// fields [`TextShapeKey::with_bound`] writes, and nothing else.
///
/// Owns the width normalization — the raw width canonicalizes to the
/// whole-px wrap grid here. Negative widths (over-constrained layouts)
/// clamp to zero; non-finite widths are a logic error, and callers gate on
/// `is_finite`.
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
    halign_q: u8,
    fit_q: u8,
}

impl WrapBound {
    pub(super) fn new(max_width_px: f32, halign: HAlign, fit: LineFit) -> Self {
        debug_assert!(max_width_px.is_finite(), "text wrap width must be finite");
        Self {
            max_w_q: quantize_width(wrap::canonical_wrap_width(max_width_px)).min(MAX_W_NONE - 1),
            halign_q: match fit {
                // Projected onto what shaping actually varies on, not
                // stored raw: `cosmic_align` maps `Auto` and `Stretch`
                // alike to "no per-line align", so keeping them apart
                // here minted two keys, two cache entries and two
                // reshapes for a byte-identical buffer. `Stretch` is a
                // box-alignment concept with no per-line meaning.
                LineFit::Wrap => match halign {
                    HAlign::Auto | HAlign::Stretch => HAlign::Auto as u8,
                    other => other as u8,
                },
                LineFit::Clip | LineFit::Ellipsis => HAlign::Auto as u8,
            },
            fit_q: fit as u8,
        }
    }
}

fn quantize_width(value: f32) -> u32 {
    (value.max(0.0) * 64.0).fast_round() as u32
}

fn quantize_metric(value: f32) -> u32 {
    quantize_width(value).max(1)
}

fn dequantize(value: u32) -> f32 {
    value as f32 / 64.0
}
