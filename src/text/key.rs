//! Canonical shaped-run identity: shaping parameters quantized into a
//! stable, purely integral cache key.

use crate::common::hash;
use crate::layout::types::align::HAlign;
use crate::primitives::approx::EPS;
use crate::primitives::interned_str::{InternedText, RecordedText, TextSource};
use crate::primitives::num::F32Ext;
use crate::text::wrap::{self, LineFit};
use crate::text::{FontFamily, FontWeight, TextShapeRequest};

pub(crate) const TEXT_METRICS_ERROR: &str =
    "font size and line height must be finite and above the UI epsilon";

pub(crate) fn text_metrics_valid(font_size_px: f32, line_height_px: f32) -> bool {
    font_size_px.is_finite()
        && font_size_px > EPS
        && line_height_px.is_finite()
        && line_height_px > EPS
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
    /// [`Self::bounded`] snaps the width to the measure cache's grid via
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

    /// Record time already rejected invalid metrics (`Shape::is_noop`,
    /// theme validation), so reaching here with them is a logic error —
    /// debug-asserted rather than re-validated on the shaping hot path.
    pub(crate) fn unbounded(
        text_hash: u64,
        font_size_px: f32,
        line_height_px: f32,
        family: FontFamily,
        weight: FontWeight,
    ) -> Self {
        debug_assert!(
            text_metrics_valid(font_size_px, line_height_px),
            "{TEXT_METRICS_ERROR}",
        );
        Self {
            text_hash: text_hash.max(1),
            size_q: quantize_metric(font_size_px),
            max_w_q: MAX_W_NONE,
            lh_q: quantize_metric(line_height_px),
            family_q: family as u8,
            weight_q: weight as u8,
            halign_q: HAlign::Auto as u8,
            fit_q: LineFit::Wrap as u8,
        }
    }

    /// Owns width normalization: the raw width canonicalizes to the whole-px
    /// wrap grid here, so every construction path mints the same identity.
    /// Negative widths (over-constrained layouts) clamp to zero; non-finite
    /// widths are a logic error — callers gate on `is_finite`.
    pub(crate) fn bounded(self, max_width_px: f32, halign: HAlign, fit: LineFit) -> Self {
        debug_assert!(max_width_px.is_finite(), "text wrap width must be finite");
        Self {
            max_w_q: quantize_width(wrap::canonical_wrap_width(max_width_px)).min(MAX_W_NONE - 1),
            halign_q: match fit {
                LineFit::Wrap => halign as u8,
                LineFit::Clip | LineFit::Ellipsis => HAlign::Auto as u8,
            },
            fit_q: fit as u8,
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
    /// exist at all; see `CosmicMeasure::measure_truncated`.
    pub(super) fn family(self) -> FontFamily {
        debug_assert!(
            self.family_q <= 1,
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
            self.weight_q <= 1,
            "invalid FontWeight tag {}",
            self.weight_q
        );
        match self.weight_q {
            0 => FontWeight::Regular,
            _ => FontWeight::Bold,
        }
    }

    pub(super) fn halign(self) -> HAlign {
        debug_assert!(self.halign_q <= 4, "invalid HAlign tag {}", self.halign_q);
        match self.halign_q {
            0 => HAlign::Auto,
            1 => HAlign::Left,
            2 => HAlign::Center,
            3 => HAlign::Right,
            _ => HAlign::Stretch,
        }
    }

    pub(super) fn fit(self) -> LineFit {
        debug_assert!(self.fit_q <= 2, "invalid LineFit tag {}", self.fit_q);
        match self.fit_q {
            0 => LineFit::Wrap,
            1 => LineFit::Clip,
            _ => LineFit::Ellipsis,
        }
    }
}

/// One shaped run's render-handoff identity: the shaped-buffer cache key
/// plus the record-store span of the exact source bytes it hashes. Minted
/// once by the encoder via [`Self::new`] (which checks the pairing against
/// the recorded content hash) and carried as a unit through the paint
/// payload, composer, and text backend so the key cannot drift from its
/// bytes between layers; [`Self::resolve_request`] is the single place the
/// pair turns back into a shaping request.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ShapedTextRef {
    pub(crate) key: TextShapeKey,
    pub(crate) source: TextSource,
}

impl ShapedTextRef {
    /// Pair a measured cache key with the recorded source it was shaped
    /// from. The O(1) hash comparison catches a mis-paired key/source at
    /// the only place both sides are still individually known.
    pub(crate) fn new(key: TextShapeKey, text: &RecordedText) -> Self {
        debug_assert_eq!(
            key.text_hash,
            text.hash.max(1),
            "shaped-text key paired with a different run's source bytes",
        );
        Self {
            key,
            source: text.source,
        }
    }

    /// Resolve the retained bytes and rebuild the shaping request the
    /// backend replays on an encoded-cache miss. Debug-checks that the
    /// resolved bytes still hash to the key's content hash — the contract
    /// that makes reusing a cached shaped buffer sound.
    pub(crate) fn resolve_request<'a>(
        self,
        interned_text: &'a InternedText<'_>,
    ) -> TextShapeRequest<'a> {
        let text = self.source.resolve(interned_text);
        debug_assert_eq!(hash::hash_str(text).max(1), self.key.text_hash);
        TextShapeRequest {
            text,
            key: self.key,
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
