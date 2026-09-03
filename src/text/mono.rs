//! Deterministic placeholder shaping for the mono fallback: every glyph
//! is `font_size_px * 0.5` wide, so a layout case can state the width it
//! expects as arithmetic rather than as whatever the bundled face
//! advances to. [`root`] and [`resolve`] are the metric
//! behind [`TextShaper::test_mono`](crate::text::shaper::TextShaper) — the
//! same two-kind split the cosmic measurer offers — and
//! [`single_line_caret_x`] / [`nearest_byte`] are the geometry
//! [`probe`] falls back to for the runs it produces.
//!
//! Only [`TextShaper::test_mono`](crate::text::shaper::TextShaper) makes
//! a run that reaches any of this, so the whole module is gated and
//! production compiles none of it.
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

/// What the mono metric lays one run out to.
#[derive(Debug)]
pub(super) struct MonoLayout {
    pub(super) size: Size,
    pub(super) single_line: bool,
}

/// Lay `request` out under the mono metric: every glyph is
/// `font_size_px * 0.5` wide and the line uses `line_height_px`; wrapping
/// is approximated by simple character-count division. At a 16 px font
/// size that is an 8 px/char × 16 px line, which is what the layout
/// tests pin.
///
/// Mints no shaped buffer, so `TextSystem` reports
/// [`TextShapeKey::INVALID`](crate::text::key::TextShapeKey::INVALID) for
/// every run measured this way and the renderer drops them cleanly.
///
/// Reached only through [`root`] and [`resolve`], which are reached only
/// through the shaper's dispatch — so the text is non-empty by the time it
/// arrives, exactly as on the cosmic side.
fn layout(request: TextShapeRequest<'_>) -> MonoLayout {
    let text = request.text;
    let font_size_px = request.key.font_size_px();
    let line_h = request.key.line_height_px();
    let max_width_px = request.key.max_width_px();
    let glyph_w = font_size_px * 0.5;
    // Mono is a deterministic stub — count one "char" per byte. Correct for
    // ASCII (which is what every test and bench uses); for multibyte input
    // it overcounts, but mono is not a production path.
    let total_chars = text.len() as f32;
    let unbroken_w = total_chars * glyph_w;
    let truncating_fit = matches!(request.key.fit(), LineFit::Clip | LineFit::Ellipsis);

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
    MonoLayout { size, single_line }
}

/// The run's unbounded shape under the mono metric — the twin of
/// [`CosmicMeasure::root`](crate::text::cosmic::CosmicMeasure).
pub(super) fn root(request: TextShapeRequest<'_>, floor: WrapFloor) -> TextRoot {
    let MonoLayout { size, single_line } = layout(request);
    // Mono has no real word boundaries, so the wrap floor falls back to
    // "the longest run of non-space bytes". Skipped entirely when the
    // caller's policy never reads it, matching the cosmic path so the two
    // agree on when the floor is absent rather than zero.
    let intrinsic_min = (floor == WrapFloor::Scan).then(|| {
        let mut longest = 0u32;
        let mut run = 0u32;
        for &b in request.text.as_bytes() {
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
        longest as f32 * request.key.font_size_px() * 0.5
    });
    TextRoot {
        size,
        intrinsic_min,
        single_line,
    }
}

/// The extent this run resolves to at its key's committed width — the
/// twin of [`CosmicMeasure::resolve`](crate::text::cosmic::CosmicMeasure).
pub(super) fn resolve(request: TextShapeRequest<'_>) -> Size {
    layout(request).size
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    /// The metric's own layout, including the single-line flag no shaper
    /// path reports off a *bounded* resolve. The mono arithmetic cases pin
    /// the metric itself, so they read it here rather than through a
    /// production entry point that would have nowhere to put it.
    pub(crate) fn layout_of(request: TextShapeRequest<'_>) -> MonoLayout {
        layout(request)
    }
}
