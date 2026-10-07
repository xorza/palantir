//! Reading geometry back off a shaped `Buffer`: a run's measured size, glyph
//! block origin, and wrap floor. Free functions so shaping paths can call them
//! while holding other measurer fields mutably.

use crate::primitives::geometry::size::Size;
use crate::text::extent::TextExtent;
use crate::text::root::TextRoot;
use crate::text::wrap::{self, WrapFloor};
use cosmic_text::Buffer;

/// Measured facts of a shaped `buffer`. Not a [`TextRoot`]: only an unbounded
/// buffer is a run's root; the unbounded paths lift it through [`Self::root`].
#[derive(Clone, Copy, Debug)]
pub(super) struct ShapedGeometry {
    /// Extent of the shaped block.
    pub(super) size: Size,
    /// Widest unbreakable segment, present exactly when `floor` asked for the
    /// scan ([`TextRoot::intrinsic_min`]).
    pub(super) intrinsic_min: Option<f32>,
    /// Whether the buffer laid out as one visual line.
    pub(super) single_line: bool,
    /// See [`CacheEntry::left`](super::cache_entry::CacheEntry::left).
    pub(super) left: f32,
}

impl ShapedGeometry {
    /// These facts as a run's unbounded shape; sound only for a buffer shaped
    /// without a width.
    pub(super) fn root(self, extent: TextExtent) -> TextRoot {
        debug_assert_eq!(extent.size, self.size, "the extent of another buffer");
        TextRoot {
            extent,
            intrinsic_min: self.intrinsic_min,
            single_line: self.single_line,
        }
    }
}

/// Measure a shaped `buffer`: the union of its lines' glyph spans (ceil'd)
/// plus, when `floor` asks, the widest unbreakable segment used as a wrap
/// floor. `breaks` is that scan's scratch.
///
/// Width is `right - left` across every line, not `right`: cosmic anchors a
/// line by alignment and direction, so `right` alone includes the leading gap.
/// Glyphless lines are skipped, or a zero-width span at 0 would drag `left` to
/// the origin.
pub(super) fn shaped_geometry(
    buffer: &Buffer,
    floor: WrapFloor,
    scratch: &mut SegmentScratch,
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
    // No glyphs anywhere (empty, or only newlines): an empty block at the origin.
    let (left, width) = if left <= right {
        (left, right - left)
    } else {
        (0.0, 0.0)
    };
    ShapedGeometry {
        size: Size::new(width.ceil(), total_h.ceil()),
        intrinsic_min: (floor == WrapFloor::Scan).then(|| intrinsic_min_width(buffer, scratch)),
        single_line: runs <= 1,
        left,
    }
}

/// Retained scratch for [`intrinsic_min_width`]: break offsets and logical-order
/// glyph indices.
#[derive(Debug, Default)]
pub(super) struct SegmentScratch {
    breaks: Vec<u32>,
    order: Vec<u32>,
}

/// Width of the widest segment no line break can split (min-content).
/// Trailing whitespace is excluded: UAX #14 breaks *after* a space, so it hangs
/// rather than widening the segment; interior non-breaking whitespace counts.
///
/// **Reported on the whole-pixel grid, rounded up.** `WrapWithOverflow` floors
/// its width here and `F32Px::canonical_px` snaps to nearest, so a raw 57.4
/// would become 57 and break the segment the floor keeps whole.
///
/// **Scanned in logical order.** Glyphs arrive visually; in a right-to-left run
/// a segment's logical first glyph (the one a break offset names) is visited
/// last, so a visual scan would merge two words.
pub(super) fn intrinsic_min_width(buffer: &Buffer, scratch: &mut SegmentScratch) -> f32 {
    let SegmentScratch { breaks, order } = scratch;
    let mut intrinsic_min = 0.0_f32;
    for run in buffer.layout_runs() {
        breaks.clear();
        breaks.extend(wrap::break_offsets(run.text));
        order.clear();
        order.extend(0..run.glyphs.len() as u32);
        // The index breaks ties without the allocation a stable sort needs.
        order.sort_unstable_by_key(|&index| (run.glyphs[index as usize].start, index));
        let mut segment_w = 0.0_f32;
        let mut trailing_ws_w = 0.0_f32;
        for &index in order.iter() {
            let g = &run.glyphs[index as usize];
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
    intrinsic_min.ceil()
}

/// Right edge (widest `x + w`) of a shaped buffer's first layout run, or
/// `0.0` when empty: the width of one line.
///
/// For the one-glyph probe [`CosmicMeasure::ellipsis_advance`](crate::text::cosmic::CosmicMeasure::ellipsis_advance)
/// shapes, whose line starts at 0. [`shaped_geometry`] spans `left..right`
/// since it also measures width-bounded buffers cosmic may anchor away from 0.
pub(super) fn first_line_right(buffer: &Buffer) -> f32 {
    buffer
        .layout_runs()
        .next()
        .and_then(|r| r.glyphs.iter().map(|g| g.x + g.w).reduce(f32::max))
        .unwrap_or(0.0)
}
