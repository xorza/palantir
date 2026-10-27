//! One shaped glyph as the truncation cut reads it, and the prefix and suffix scans that spend a width budget over them. The cut runs against the cached unbounded shape, not a reshape per width; [`CosmicMeasure::shape_truncated`](crate::text::cosmic::CosmicMeasure::shape_truncated) records why cosmic's `set_ellipsize` was reverted.

use std::ops::Range;

/// One shaped glyph reduced to what the truncation cut reads: the source
/// bytes it covers and the advance it costs.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ClusterGlyph {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) advance: f32,
}

/// Longest logical byte prefix of `glyphs` whose advances sum within `avail` and stay below `max_end`; sorts `glyphs` into logical order in place. Takes a snapshot so the cut is testable with hand-built advances.
///
/// The result is strictly below `max_end`, so feeding back the previous answer retires a cluster and the back-off in [`CosmicMeasure::shape_truncated`](crate::text::cosmic::CosmicMeasure::shape_truncated) terminates.
///
/// Glyphs arrive in visual order; summing advances in logical order makes `text[..cut]` the fitting prefix for RTL too.
///
/// A cluster can shape to several glyphs sharing a byte range, so the prefix advances past it only once all are paid for.
pub(crate) fn fitting_prefix(glyphs: &mut [ClusterGlyph], avail: f32, max_end: usize) -> usize {
    // Visual order is logical order for LTR and back-off rounds re-read a sorted slice, so skip the sort: it dominated on long single-line runs.
    if !glyphs.is_sorted_by_key(|g| g.start) {
        glyphs.sort_unstable_by_key(|g| g.start);
    }
    let mut cut = 0usize;
    let mut used = 0.0_f32;
    for (pos, g) in glyphs.iter().enumerate() {
        // Ends are non-decreasing in logical order, so once one reaches the bound none later can commit.
        if g.end >= max_end {
            break;
        }
        used += g.advance;
        if used > avail {
            break;
        }
        let cluster_paid = glyphs.get(pos + 1).is_none_or(|next| next.start >= g.end);
        if cluster_paid {
            cut = g.end;
        }
    }
    cut
}

/// Lowest logical byte start at or above `min_start` whose suffix of `glyphs` sums within `avail`, or `None` when not one cluster fits. `glyphs` are in logical order, as [`fitting_prefix`] leaves them.
///
/// Feeding back the previous answer plus one retires a cluster, so the middle cut's back-off in [`CosmicMeasure::shape_truncated`](crate::text::cosmic::CosmicMeasure::shape_truncated) terminates.
pub(crate) fn fitting_suffix(
    glyphs: &[ClusterGlyph],
    avail: f32,
    min_start: usize,
) -> Option<usize> {
    debug_assert!(glyphs.is_sorted_by_key(|g| g.start));
    let mut tail = None;
    let mut used = 0.0_f32;
    for (pos, g) in glyphs.iter().enumerate().rev() {
        if g.start < min_start {
            break;
        }
        used += g.advance;
        if used > avail {
            break;
        }
        let cluster_paid = pos
            .checked_sub(1)
            .is_none_or(|prev| glyphs[prev].end <= g.start);
        if cluster_paid {
            tail = Some(g.start);
        }
    }
    tail
}

/// Summed advance of the logically sorted `glyphs` that lie wholly inside `bytes`.
pub(crate) fn span_advance(glyphs: &[ClusterGlyph], bytes: Range<usize>) -> f32 {
    glyphs
        .iter()
        .filter(|g| g.start >= bytes.start && g.end <= bytes.end)
        .map(|g| g.advance)
        .sum()
}
