//! Reading geometry back off a shaped `Buffer`: what a run measured to,
//! where its glyph block starts, and the wrap floor a segment scan finds.
//!
//! Free functions over a borrowed buffer rather than methods, because
//! none of them needs the measurer — which is what lets the shaping paths
//! call them while holding other fields of it mutably.

use crate::primitives::size::Size;
use crate::text::root::TextRoot;
use crate::text::wrap::WrapFloor;
use cosmic_text::Buffer;

/// Measured geometry of a shaped `buffer`: the run's own extent plus the
/// block origin every reader normalizes against.
#[derive(Clone, Copy, Debug)]
pub(super) struct ShapedGeometry {
    pub(super) root: TextRoot,
    /// See [`CacheEntry::left`].
    pub(super) left: f32,
}

/// Measure a shaped `buffer`: the union of its lines' glyph spans (ceil'd)
/// plus, when `floor` asks for it, the widest unbreakable segment the wrap
/// path uses as a floor once a parent commits a narrower width. `breaks`
/// is that scan's scratch, untouched when it is skipped.
///
/// Width is `right - left` across every line, not `right` alone. Cosmic
/// anchors a line wherever its alignment and direction put it, so the
/// distance from 0 is the run's width *plus* whatever gap precedes it;
/// spanning the union measures the glyphs and nothing else. Taking both
/// edges per line also subsumes the RTL case a trailing-edge scan needed
/// a `max` for: a right-to-left run's last glyph is its leftmost.
///
/// Glyphless lines are skipped rather than contributing a zero-width span
/// at 0, which would drag `left` back to the origin for a block that
/// starts elsewhere.
pub(super) fn shaped_geometry(
    buffer: &Buffer,
    floor: WrapFloor,
    breaks: &mut Vec<u32>,
) -> ShapedGeometry {
    let mut left = f32::INFINITY;
    let mut right = f32::NEG_INFINITY;
    let mut total_h = 0.0_f32;
    let mut runs = 0usize;
    for run in buffer.layout_runs() {
        runs += 1;
        total_h = total_h.max(run.line_top + run.line_height);
        for glyph in run.glyphs {
            left = left.min(glyph.x);
            right = right.max(glyph.x + glyph.w);
        }
    }
    // No glyphs anywhere — an empty buffer, or one holding only newlines.
    // The block is empty and sits at the origin.
    let (left, width) = if left <= right {
        (left, right - left)
    } else {
        (0.0, 0.0)
    };
    ShapedGeometry {
        root: TextRoot {
            size: Size::new(width.ceil(), total_h.ceil()),
            intrinsic_min: (floor == WrapFloor::Scan).then(|| intrinsic_min_width(buffer, breaks)),
            single_line: runs <= 1,
        },
        left,
    }
}

/// Byte offsets in `text` that start a new unbreakable segment, i.e. the
/// UAX #14 opportunities minus the terminal one at `text.len()`, which
/// ends the text rather than opening a segment.
///
/// Same source cosmic-text splits its shape words on
/// (`cosmic-text/src/shape.rs`), so the wrap floor this feeds cannot
/// claim a segment the shaper would happily break.
fn collect_break_offsets(text: &str, out: &mut Vec<u32>) {
    out.clear();
    out.extend(
        unicode_linebreak::linebreaks(text)
            .map(|(offset, _)| offset)
            .filter(|&offset| offset < text.len())
            .map(|offset| offset as u32),
    );
}

/// Width of the widest segment no line break can split — the min-content
/// width. Trailing whitespace is excluded because UAX #14 places its
/// break opportunity *after* a space, so a space always ends its segment
/// and hangs rather than widening it; interior non-breaking whitespace
/// (U+00A0 and friends) opens no opportunity and so counts in full.
pub(super) fn intrinsic_min_width(buffer: &Buffer, breaks: &mut Vec<u32>) -> f32 {
    let mut intrinsic_min = 0.0_f32;
    for run in buffer.layout_runs() {
        collect_break_offsets(run.text, breaks);
        let mut segment_w = 0.0_f32;
        let mut trailing_ws_w = 0.0_f32;
        for g in run.glyphs {
            // Glyphs arrive in visual order, but a segment's glyphs stay
            // contiguous within a level run, so entering a new segment
            // closes the previous one whichever way the run reads.
            if breaks.binary_search(&(g.start as u32)).is_ok() {
                intrinsic_min = intrinsic_min.max(segment_w);
                segment_w = 0.0;
                trailing_ws_w = 0.0;
            }
            if run.text[g.start..g.end].chars().all(char::is_whitespace) {
                trailing_ws_w += g.w;
            } else {
                segment_w += trailing_ws_w + g.w;
                trailing_ws_w = 0.0;
            }
        }
        intrinsic_min = intrinsic_min.max(segment_w);
    }
    intrinsic_min
}
