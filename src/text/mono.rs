//! Deterministic placeholder shaping for the mono fallback: every glyph is `font_size * 0.5` wide, so layout tests state widths as arithmetic. Reached only through [`TextShaper::test_mono`](crate::text::shaper::TextShaper), so the module is test-gated.

use crate::primitives::geometry::size::Size;
use crate::text::extent::TextExtent;
use crate::text::request::TextShapeRequest;
use crate::text::root::TextRoot;
use crate::text::wrap::{self, LineFit, WrapFloor};

/// Width of one `char` at `font_size`, whatever its UTF-8 length.
fn glyph_width(font_size: f32) -> f32 {
    font_size * 0.5
}

/// Caret-x along a single-line mono layout: one glyph width per `char` before `byte_offset`.
pub(super) fn single_line_caret_x(text: &str, byte_offset: usize, font_size: f32) -> f32 {
    let clamped = text.floor_char_boundary(byte_offset.min(text.len()));
    text[..clamped].chars().count() as f32 * glyph_width(font_size)
}

/// Inverse of [`single_line_caret_x`]: the char boundary whose prefix-x is closest to `target_x`.
pub(super) fn nearest_byte(text: &str, target_x: f32, font_size: f32) -> usize {
    let mut best_off = 0usize;
    let mut best_dist = target_x.abs();
    for (i, ch) in text.char_indices() {
        let next = i + ch.len_utf8();
        let x = single_line_caret_x(text, next, font_size);
        let d = (x - target_x).abs();
        if d < best_dist {
            best_dist = d;
            best_off = next;
        }
    }
    best_off
}

/// The run's unbounded shape under the mono metric (twin of [`CosmicMeasure::root`](crate::text::cosmic::CosmicMeasure)); mints no shaped buffer, so the renderer drops these runs.
pub(super) fn root(request: TextShapeRequest<'_>, floor: WrapFloor) -> TextRoot {
    let glyph_w = glyph_width(request.key.font_size());
    TextRoot {
        // Mono reads no outlines: a cell holds its glyph.
        extent: TextExtent::inked_within(Size::new(
            request.text.chars().count() as f32 * glyph_w,
            request.key.line_height(),
        )),
        intrinsic_min: (floor == WrapFloor::Scan)
            .then(|| intrinsic_min_width(request.text, glyph_w)),
        // Mono breaks no lines of its own: an unbounded run is one line.
        single_line: true,
    }
}

/// The extent this run resolves to at its key's committed width; twin of [`CosmicMeasure::resolve`](crate::text::cosmic::CosmicMeasure), routed by [`LineFit`].
pub(super) fn resolve(request: TextShapeRequest<'_>) -> TextExtent {
    let key = request.key;
    let glyph_w = glyph_width(key.font_size());
    let line_h = key.line_height();
    let max = key.max_width().expect("a bounded resolve commits a width");
    let chars = request.text.chars().count() as f32;
    let unbroken_w = chars * glyph_w;
    TextExtent::inked_within(match key.fit() {
        // One line capped at the width, matching the cosmic cut.
        LineFit::Clip | LineFit::Ellipsis => Size::new(unbroken_w.min(max), line_h),
        // Wrapping is approximated by character-count division (8 px/char at 16 px).
        LineFit::Wrap => {
            let per_line = (max / glyph_w).floor().max(1.0);
            let lines = (chars / per_line).ceil().max(1.0);
            Size::new((per_line * glyph_w).min(unbroken_w), lines * line_h)
        }
    })
}

/// Widest unbreakable segment of `text` under a uniform `glyph_w`; twin of `cosmic::geometry::intrinsic_min_width`. Segments come from [`wrap::break_offsets`] with trailing whitespace dropped (the break sits after a space). Ceil'd like its twin: `WrapWithOverflow` floors a committed width at this.
fn intrinsic_min_width(text: &str, glyph_w: f32) -> f32 {
    let mut widest = 0usize;
    let mut start = 0usize;
    for next in wrap::break_offsets(text) {
        let next = next as usize;
        widest = widest.max(text[start..next].trim_end().chars().count());
        start = next;
    }
    widest = widest.max(text[start..].trim_end().chars().count());
    (widest as f32 * glyph_w).ceil()
}
