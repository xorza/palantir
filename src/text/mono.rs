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
use crate::text::wrap::{self, LineFit, WrapFloor};

/// Width of one byte at `font_size_px`. Mono counts one "char" per byte:
/// correct for the ASCII every test and bench uses, and an overcount for
/// multibyte input that no production path meets.
fn glyph_width(font_size_px: f32) -> f32 {
    font_size_px * 0.5
}

/// Caret-x along a single-line mono layout (0.5×font_size per byte).
/// Multi-line aware callers should go through `cursor_xy` instead —
/// this is the cheap path for the mono fallback's degenerate single-
/// line behaviour.
pub(super) fn single_line_caret_x(text: &str, byte_offset: usize, font_size_px: f32) -> f32 {
    let clamped = byte_offset.min(text.len());
    (clamped as f32) * glyph_width(font_size_px)
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

/// The run's unbounded shape under the mono metric — the twin of
/// [`CosmicMeasure::root`](crate::text::cosmic::CosmicMeasure).
///
/// Mints no shaped buffer, so `TextSystem` reports
/// [`TextShapeKey::INVALID`](crate::text::key::TextShapeKey::INVALID) for
/// every run measured this way and the renderer drops them cleanly.
///
/// Reached only through the shaper's dispatch, so the text is non-empty
/// and the key commits no width by the time it arrives — exactly as on
/// the cosmic side.
pub(super) fn root(request: TextShapeRequest<'_>, floor: WrapFloor) -> TextRoot {
    let glyph_w = glyph_width(request.key.font_size_px());
    TextRoot {
        size: Size::new(
            request.text.len() as f32 * glyph_w,
            request.key.line_height_px(),
        ),
        intrinsic_min: (floor == WrapFloor::Scan)
            .then(|| intrinsic_min_width(request.text, glyph_w)),
        // Mono breaks no lines of its own: an unbounded run is one line
        // however many newlines it holds.
        single_line: true,
    }
}

/// The extent this run resolves to at its key's committed width — the
/// twin of [`CosmicMeasure::resolve`](crate::text::cosmic::CosmicMeasure),
/// routed by the same [`LineFit`] to the same two answers.
pub(super) fn resolve(request: TextShapeRequest<'_>) -> Size {
    let key = request.key;
    let glyph_w = glyph_width(key.font_size_px());
    let line_h = key.line_height_px();
    let max = key
        .max_width_px()
        .expect("a bounded resolve commits a width");
    let chars = request.text.len() as f32;
    let unbroken_w = chars * glyph_w;
    match key.fit() {
        // One line capped at the width, which is what the cosmic side's
        // cut measures to once it has retired the clusters that overrun.
        LineFit::Clip | LineFit::Ellipsis => Size::new(unbroken_w.min(max), line_h),
        // Wrapping is approximated by character-count division: at a
        // 16 px font size, an 8 px/char × 16 px line.
        LineFit::Wrap => {
            let per_line = (max / glyph_w).floor().max(1.0);
            let lines = (chars / per_line).ceil().max(1.0);
            Size::new((per_line * glyph_w).min(unbroken_w), lines * line_h)
        }
    }
}

/// Widest unbreakable segment of `text` under a uniform `glyph_w` — the
/// twin of
/// [`geometry::intrinsic_min_width`](crate::text::cosmic::geometry::intrinsic_min_width),
/// which answers the same question off a shaped buffer's glyph widths.
///
/// Segments come from [`wrap::break_offsets`], the rule both twins
/// measure against, and each drops its trailing whitespace for the same
/// reason: the break opportunity sits *after* a space, so a space hangs
/// off the end of its segment rather than widening it.
///
/// Ceil'd like its twin — `WrapWithOverflow` floors a committed width at
/// this, and rounding down would break the very segment the floor exists
/// to keep whole.
fn intrinsic_min_width(text: &str, glyph_w: f32) -> f32 {
    let mut widest = 0usize;
    let mut start = 0usize;
    for next in wrap::break_offsets(text) {
        let next = next as usize;
        widest = widest.max(text[start..next].trim_end().len());
        start = next;
    }
    widest = widest.max(text[start..].trim_end().len());
    (widest as f32 * glyph_w).ceil()
}
