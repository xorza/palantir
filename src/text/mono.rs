//! Deterministic placeholder shaping for the mono fallback: every glyph is
//! `font_size_px * 0.5` wide, so the engine can run in tests and headless
//! tools without a font system. [`measure`] is the metric behind
//! [`TextShaper::test_mono`](crate::text::shaper::TextShaper), and
//! [`single_line_caret_x`] / [`nearest_byte`] are the geometry
//! [`probe`] falls back to for the runs it produces.
//!
//! Only [`TextShaper::test_mono`](crate::text::shaper::TextShaper) makes
//! a run that reaches any of this, so the whole module is gated and
//! production compiles none of it. A production build *does* reach the
//! no-shaped-buffer case — empty text is unshaped everywhere — but the
//! answer there is a constant, so it belongs with the caller that knows
//! the text is empty rather than behind a gate here.
//!
//! [`probe`]: crate::text::probe

use crate::primitives::size::Size;
use crate::text::request::TextShapeRequest;
use crate::text::root::TextRoot;
use crate::text::wrap::{LineFit, WrapFloor};

/// Caret-x along a single-line mono layout (0.5×font_size per byte).
/// Multi-line aware callers should go through `cursor_xy` instead —
/// this is the cheap path for the mono fallback's degenerate single-
/// line behaviour.
pub(super) fn single_line_caret_x(text: &str, byte_offset: usize, font_size_px: f32) -> f32 {
    let clamped = byte_offset.min(text.len());
    (clamped as f32) * font_size_px * 0.5
}

/// Inverse of [`single_line_caret_x`]. Picks the char boundary whose
/// prefix-x is closest to `target_x` so click positioning on the mono
/// fallback matches the rendered glyph layout exactly.
pub(super) fn nearest_byte(text: &str, target_x: f32, font_size_px: f32) -> usize {
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
/// [`TextShapeKey::INVALID`](crate::text::key::TextShapeKey::INVALID) for
/// every run measured this way and the
/// renderer drops them cleanly.
pub(crate) fn measure(request: TextShapeRequest<'_>, floor: WrapFloor) -> TextRoot {
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
    // non-space bytes" as the wrap floor. Skipped entirely when the
    // caller's policy never reads it, matching the cosmic path so the
    // two agree on when the floor is absent rather than zero.
    let intrinsic_min = if floor == WrapFloor::Skip {
        None
    } else if truncating_fit {
        Some(0.0)
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
        Some(longest as f32 * glyph_w)
    };
    TextRoot {
        size,
        intrinsic_min,
        single_line,
    }
}
