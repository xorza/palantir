//! Deterministic placeholder shaping for the mono fallback: every glyph is
//! `font_size_px * 0.5` wide, so the engine can run in tests and headless
//! tools without a font system. [`test_support::measure`] is the metric
//! behind [`TextShaper::test_mono`](crate::text::TextShaper).
//!
//! [`caret_x`] and [`byte_at_x`] are the geometry [`probe`] falls back to
//! when a layout has no shaped buffer, and they compile in every build:
//! empty text is unshaped in production too, and it is answered here so
//! the caller carries no `cfg`. Everything past that point — a *non-empty*
//! run with no shaped buffer — exists only under the mono shaper, so the
//! layout itself lives in the gated [`test_support`] and production builds
//! reach an unreachable branch instead.
//!
//! [`probe`]: crate::text::probe

/// Caret-x for a run with no shaped buffer. Empty text has no glyphs to
/// walk in any build and takes `empty_x` — the caller's alignment-aware
/// empty-line placement.
pub(crate) fn caret_x(text: &str, byte_offset: usize, font_size_px: f32, empty_x: f32) -> f32 {
    if text.is_empty() {
        return empty_x;
    }
    #[cfg(any(test, feature = "internals"))]
    {
        test_support::single_line_caret_x(text, byte_offset, font_size_px)
    }
    #[cfg(not(any(test, feature = "internals")))]
    {
        let _ = (byte_offset, font_size_px);
        unreachable!("a non-empty run without a shaped buffer requires the mono shaper")
    }
}

/// Byte offset nearest `target_x` for a run with no shaped buffer. Empty
/// text has exactly one position.
pub(crate) fn byte_at_x(text: &str, target_x: f32, font_size_px: f32) -> usize {
    // Outside the `cfg` deliberately: empty text is the one unshaped case
    // production reaches, so it must be answered before the mono-only
    // layout below is gated away.
    if text.is_empty() {
        return 0;
    }
    #[cfg(any(test, feature = "internals"))]
    {
        test_support::nearest_byte(text, target_x, font_size_px)
    }
    #[cfg(not(any(test, feature = "internals")))]
    {
        let _ = (target_x, font_size_px);
        unreachable!("a non-empty run without a shaped buffer requires the mono shaper")
    }
}

/// The mono layout itself. Only [`TextShaper::test_mono`] produces runs
/// that reach it, so production builds compile none of this.
///
/// [`TextShaper::test_mono`]: crate::text::TextShaper
#[cfg(any(test, feature = "internals"))]
pub(crate) mod test_support {
    use crate::primitives::size::Size;
    use crate::text::wrap::LineFit;
    use crate::text::{TextRoot, TextShapeRequest};

    /// Caret-x along a single-line mono layout (0.5×font_size per byte).
    /// Multi-line aware callers should go through `cursor_xy` instead —
    /// this is the cheap path for the mono fallback's degenerate single-
    /// line behaviour.
    pub(crate) fn single_line_caret_x(text: &str, byte_offset: usize, font_size_px: f32) -> f32 {
        let clamped = byte_offset.min(text.len());
        (clamped as f32) * font_size_px * 0.5
    }

    /// Inverse of [`single_line_caret_x`]. Picks the char boundary whose
    /// prefix-x is closest to `target_x` so click positioning on the mono
    /// fallback matches the rendered glyph layout exactly.
    pub(crate) fn nearest_byte(text: &str, target_x: f32, font_size_px: f32) -> usize {
        let mut best_off = 0usize;
        let mut best_dist = target_x.abs();
        for (i, ch) in text.char_indices() {
            let next = i + ch.len_utf8();
            let x = single_line_caret_x(text, next, font_size_px);
            let d = (x - target_x).abs();
            if d < best_dist {
                best_dist = d;
                best_off = next;
            }
        }
        best_off
    }

    /// Deterministic placeholder metric behind `TextShaper::test_mono`.
    /// Every glyph is `font_size_px * 0.5` wide and the line uses
    /// `line_height_px`; wrapping is approximated by simple
    /// character-count division. At the historical 16 px font size this is the
    /// 8 px/char × 16 px line layout the engine was hard-coded to before text
    /// shaping landed, which is what existing layout tests pin.
    ///
    /// Mints no shaped buffer, so `TextSystem` reports
    /// [`TextShapeKey::INVALID`] for every run measured this way and the
    /// renderer drops them cleanly.
    pub(crate) fn measure(request: TextShapeRequest<'_>) -> TextRoot {
        let text = request.text;
        if text.is_empty() {
            return TextRoot::ZERO;
        }
        let font_size_px = request.key.font_size_px();
        let line_height_px = request.key.line_height_px();
        let max_width_px = request.key.max_width_px();
        let fit = request.key.fit();
        let glyph_w = font_size_px * 0.5;
        let line_h = line_height_px;
        // Mono is a deterministic stub — count one "char" per byte. Correct for
        // ASCII (which is what every test and bench uses); for multibyte input
        // it overcounts, but mono is not a production path.
        let total_chars = text.len() as f32;
        let unbroken_w = total_chars * glyph_w;
        let truncating_fit = matches!(fit, LineFit::Clip | LineFit::Ellipsis);

        let (size, single_line) = match max_width_px {
            None => (Size::new(unbroken_w, line_h), true),
            Some(max) if max >= unbroken_w => (Size::new(unbroken_w, line_h), true),
            // Clip/ellipsis is one line capped at the available width.
            Some(max) if truncating_fit => (Size::new(max, line_h), true),
            Some(max) => {
                let chars_per_line = (max / glyph_w).floor().max(1.0);
                let lines = (total_chars / chars_per_line).ceil().max(1.0);
                (
                    Size::new((chars_per_line * glyph_w).min(unbroken_w), lines * line_h),
                    lines <= 1.0,
                )
            }
        };
        // A truncated run shrinks to nothing — zero floor. Otherwise mono has
        // no real word boundaries, so fall back to "the longest run of
        // non-space bytes" as the wrap floor.
        let intrinsic_min = if truncating_fit {
            0.0
        } else {
            let mut longest = 0u32;
            let mut run = 0u32;
            for &b in text.as_bytes() {
                if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
                    if run > longest {
                        longest = run;
                    }
                    run = 0;
                } else {
                    run += 1;
                }
            }
            if run > longest {
                longest = run;
            }
            longest as f32 * glyph_w
        };
        TextRoot {
            size,
            intrinsic_min,
            single_line,
        }
    }
}
