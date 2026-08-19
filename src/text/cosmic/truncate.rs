//! Cluster-precise truncation: cutting a run to a committed width, with
//! or without a trailing ellipsis.
//!
//! The cut is measured against the cached *unbounded* shape rather than
//! by reshaping the whole string per width, which is what keeps a resize
//! drag cheap — [`CosmicMeasure::measure_truncated`] carries the
//! measurement that settled it, including why delegating to cosmic's own
//! `set_ellipsize` was written, benchmarked and reverted.

use crate::text::cosmic::cache_entry::CacheEntry;
use crate::text::key::TextShapeKey;
use crate::text::{FontFamily, FontWeight};
use cosmic_text::Buffer;
use rustc_hash::FxHashMap;

/// The cached unbounded shape a truncating fit cuts from.
///
/// [`CosmicMeasure::measure_truncated`] calls
/// [`CosmicMeasure::ensure_buffer`] on this key before reaching for it,
/// and re-reads it once per back-off round because the shaping in
/// between needs `&mut self`, so the borrow cannot be held across the
/// loop.
///
/// Hands back the whole entry rather than its buffer: the caller wants
/// the measured [`TextRoot`] as well as the glyphs, and both come out of
/// the one lookup.
///
/// Takes the map rather than `&self` on purpose: the caller holds
/// `&mut self.logical_order` at the same time, and only a borrow of the
/// one field stays disjoint from it.
#[inline]
pub(super) fn truncation_probe(
    cache: &FxHashMap<TextShapeKey, CacheEntry>,
    key: TextShapeKey,
) -> &CacheEntry {
    cache
        .get(&key)
        .expect("truncation requires the cached unbounded shape")
}

/// Right edge (widest `x + w` across glyphs — an RTL run's last glyph is
/// its leftmost) of a shaped buffer's first layout run, or `0.0` when
/// empty — the rendered width of one line.
///
/// For the one-glyph unbounded probe [`CosmicMeasure::ellipsis_advance`]
/// shapes, whose line starts at 0, the right edge is the width.
/// [`shaped_geometry`] spans `left..right` instead because it also
/// measures width-bounded buffers, which cosmic may anchor away from the
/// origin — and every caller that has a measured [`TextRoot`] to hand
/// reads `size.w` off that rather than walking glyphs again.
pub(super) fn first_line_right(buffer: &Buffer) -> f32 {
    buffer
        .layout_runs()
        .next()
        .and_then(|r| r.glyphs.iter().map(|g| g.x + g.w).reduce(f32::max))
        .unwrap_or(0.0)
}

/// Memoized trailing advance of "…" for one face.
///
/// `Default` only to satisfy `tinyvec`'s `Array` bound — it fills the
/// unused tail of [`CosmicMeasure::ellipsis`], which `len` keeps out of
/// every read. A zeroed memo could not match a live face anyway:
/// `quantize_metric` floors `size_q` at 1.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct EllipsisMemo {
    size_q: u32,
    family_q: u8,
    weight_q: u8,
    advance: f32,
}

impl EllipsisMemo {
    /// A face to look up, with no advance measured for it yet.
    pub(super) fn wanted(size_q: u32, family: FontFamily, weight: FontWeight) -> Self {
        Self {
            size_q,
            family_q: family as u8,
            weight_q: weight as u8,
            advance: 0.0,
        }
    }

    /// This memo's advance, if it was shaped from the same face at the
    /// same size as `want`. `None` is the miss that makes the caller
    /// shape one.
    pub(super) fn advance_for(&self, want: &Self) -> Option<f32> {
        (self.size_q == want.size_q
            && self.family_q == want.family_q
            && self.weight_q == want.weight_q)
            .then_some(self.advance)
    }

    /// `want` with the advance that was just measured for it.
    pub(super) fn measured(self, advance: f32) -> Self {
        Self { advance, ..self }
    }
}

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
/// [`CosmicMeasure::measure_truncated`]'s back-off terminate. Pass
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
