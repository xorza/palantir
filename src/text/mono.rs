//! Deterministic placeholder shaping used when no
//! [`CosmicMeasure`](crate::text::cosmic::CosmicMeasure) is installed:
//! every glyph is `font_size_px * 0.5` wide, so the engine can run in
//! tests and headless tools without a font system.

use crate::primitives::size::Size;
use crate::text::key::{LineFit, TextShapeKey};
use crate::text::{TextMeasurement, TextShapeRequest};

/// Deterministic placeholder metric used when [`crate::Ui`] has no
/// cosmic shaper installed. Every glyph is `font_size_px * 0.5` wide and
/// the line uses `line_height_px`; wrapping is approximated by simple
/// character-count division. At the historical 16 px font size this is the
/// 8 px/char × 16 px line layout the engine was hard-coded to before text
/// shaping landed, which is what existing layout tests pin.
///
/// Always returns [`TextShapeKey::INVALID`] — there's no shaped buffer to
/// look up, so the renderer drops these runs cleanly.
pub(crate) fn measure(request: TextShapeRequest<'_>) -> TextMeasurement {
    let text = request.text;
    if text.is_empty() {
        return TextMeasurement::ZERO;
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
    let single_line = matches!(fit, LineFit::Clip | LineFit::Ellipsis);

    let size = match max_width_px {
        None => Size::new(unbroken_w, line_h),
        Some(max) if max >= unbroken_w => Size::new(unbroken_w, line_h),
        // Clip/ellipsis is one line capped at the available width.
        Some(max) if single_line => Size::new(max, line_h),
        Some(max) => {
            let chars_per_line = (max / glyph_w).floor().max(1.0);
            let lines = (total_chars / chars_per_line).ceil().max(1.0);
            Size::new((chars_per_line * glyph_w).min(unbroken_w), lines * line_h)
        }
    };
    // A truncated run shrinks to nothing — zero floor. Otherwise mono has
    // no real word boundaries, so fall back to "the longest run of
    // non-space bytes" as the wrap floor.
    let intrinsic_min = if single_line {
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
    TextMeasurement {
        size,
        key: TextShapeKey::INVALID,
        intrinsic_min,
    }
}

/// Caret-x along a single-line mono layout (0.5×font_size per byte).
/// Multi-line aware callers should go through `cursor_xy` instead —
/// this is the cheap path for the mono fallback's degenerate single-
/// line behaviour.
pub(crate) fn caret_x_single_line(text: &str, byte_offset: usize, font_size_px: f32) -> f32 {
    let clamped = byte_offset.min(text.len());
    (clamped as f32) * font_size_px * 0.5
}

/// Inverse of [`caret_x_single_line`]. Picks the char boundary
/// whose prefix-x is closest to `target_x` so click positioning on
/// the mono fallback matches the rendered glyph layout exactly.
pub(crate) fn byte_at_x(text: &str, target_x: f32, font_size_px: f32) -> usize {
    let mut best_off = 0usize;
    let mut best_dist = target_x.abs();
    for (i, ch) in text.char_indices() {
        let next = i + ch.len_utf8();
        let x = caret_x_single_line(text, next, font_size_px);
        let d = (x - target_x).abs();
        if d < best_dist {
            best_dist = d;
            best_off = next;
        }
    }
    best_off
}
