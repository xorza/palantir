//! One shaped glyph as the truncation cut reads it, and the prefix scan
//! that spends a width budget over a run of them.
//!
//! The cut is measured against the cached *unbounded* shape rather than
//! by reshaping the whole string per width, which is what keeps a resize
//! drag cheap — [`CosmicMeasure::shape_truncated`](crate::text::cosmic::CosmicMeasure::shape_truncated)
//! carries the measurement that settled it, including why delegating to
//! cosmic's own `set_ellipsize` was written, benchmarked and reverted.

/// One shaped glyph reduced to what the truncation cut reads: the source
/// bytes it covers and the advance it costs.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ClusterGlyph {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) advance: f32,
}

/// Longest logical byte prefix of `count` shaped glyphs whose advances sum
/// within `avail` and stay below `max_end`. `order` is retained scratch,
/// refilled with the glyph indices sorted into logical order; `glyph` reads
/// one glyph by its original (visual-order) index.
///
/// Reading through `glyph` rather than taking `&[LayoutGlyph]` buys two
/// things: the caller keeps its borrow of the cache disjoint from its
/// borrow of `order`, and the cut can be unit-tested against hand-built
/// advances instead of whatever the installed fonts happen to measure.
///
/// The result is always strictly below `max_end`, so passing the previous
/// answer retires at least one more cluster — that is what makes
/// [`CosmicMeasure::shape_truncated`](crate::text::cosmic::CosmicMeasure::shape_truncated)'s back-off terminate. Pass
/// `usize::MAX` for an unbounded first cut.
///
/// Glyphs arrive in visual order, so a glyph's `x` follows the reading
/// direction rather than the logical prefix — an RTL run's first glyph sits
/// at the *right* edge and its trailing edges descend. Summing advances in
/// logical order instead makes `text[..cut]` the prefix that fits whichever
/// way the run reads.
///
/// One grapheme cluster can shape to several glyphs sharing a byte range
/// (flag and ZWJ emoji, Indic conjuncts), so the prefix only advances past a
/// cluster once every glyph covering it is paid for. Committing mid-cluster
/// would claim bytes whose advance the budget never covered, and the prefix
/// would reshape wider than `avail`.
pub(crate) fn fitting_prefix(
    count: usize,
    glyph: impl Fn(usize) -> ClusterGlyph,
    order: &mut Vec<u32>,
    avail: f32,
    max_end: usize,
) -> usize {
    order.clear();
    order.extend(0..count as u32);
    // Visual order *is* logical order for an LTR run, which is nearly
    // every run this shapes. Checking costs one key call per glyph;
    // sorting costs `n log n` of them, since `sort_unstable_by_key`
    // re-invokes the key rather than caching it. The cut itself only
    // ever reads a short prefix, so on a long single-line run — a file
    // path, a log line — the skipped sort was the dominant term.
    if !order.is_sorted_by_key(|&i| glyph(i as usize).start) {
        order.sort_unstable_by_key(|&i| glyph(i as usize).start);
    }
    let mut cut = 0usize;
    let mut used = 0.0_f32;
    for (pos, &i) in order.iter().enumerate() {
        let g = glyph(i as usize);
        // Ends are non-decreasing in logical order, so once one reaches the
        // bound no later glyph can be committed either.
        if g.end >= max_end {
            break;
        }
        used += g.advance;
        if used > avail {
            break;
        }
        let cluster_paid = order
            .get(pos + 1)
            .is_none_or(|&next| glyph(next as usize).start >= g.end);
        if cluster_paid {
            cut = g.end;
        }
    }
    cut
}
