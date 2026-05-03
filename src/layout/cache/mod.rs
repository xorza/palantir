//! Cross-frame measure cache (Phase 2: full subtree skip). Skip a
//! node's *entire subtree* — body and recursion — when its
//! `subtree_hash` and incoming `available` size both match last
//! frame. See `docs/measure-cache.md`.
//!
//! Keyed by stable [`WidgetId`]; lifecycle mirrors [`crate::ui::Damage`]
//! and [`crate::text::TextMeasurer`] — eviction piggy-backs on the
//! `SeenIds.removed()` diff at frame end.

use crate::layout::result::ShapedText;
use crate::primitives::{Size, WidgetId};
use crate::tree::NodeHash;
use rustc_hash::FxHashMap;

/// Snapshot of one node's whole measured subtree. Stored across
/// frames so a cache hit can replay every descendant's `desired`
/// (and any text shapes) with one map lookup + two slice copies,
/// skipping the recursive measure walk entirely.
///
/// `desired` and `text_shapes` are stored in pre-order, NodeId-relative
/// to the snapshot's root: index `0` is the root, indices `1..` are
/// the descendants in the same order they appear in the tree's
/// pre-order arena. Length is `subtree_end[root] - root.index()`. On
/// restore, the slices are blitted into `LayoutEngine.desired` and
/// `LayoutResult.text_shapes` at `[root.index() .. subtree_end[root]]`.
///
/// `Default` produces empty `Vec`s; the steady-state path overwrites
/// via `clear() + extend_from_slice` to keep capacity across frames.
#[derive(Default, Debug)]
pub(crate) struct SubtreeSnapshot {
    /// Rolled subtree hash from last frame. The rollup includes child
    /// count and per-child subtree hashes, so any structural or
    /// authoring change anywhere in the subtree busts the key.
    pub subtree_hash: NodeHash,
    /// `available` size passed to this node's measure last frame,
    /// quantized to integer logical pixels.
    pub available_q: AvailableKey,
    /// Outer (margin-inclusive) `desired` per descendant in subtree
    /// pre-order. `desired[0]` is the snapshot root's own size — the
    /// value `measure` returns on a hit.
    pub desired: Vec<Size>,
    /// Text-shape result per descendant, same indexing as `desired`.
    /// `None` for nodes that didn't shape text last frame.
    pub text_shapes: Vec<Option<ShapedText>>,
}

/// Quantized (`available.w`, `available.h`) used as the cache's
/// dimensional key. `i32::MAX` represents an infinite axis (ZStack /
/// Hug parents propagate `f32::INFINITY`).
pub(crate) type AvailableKey = (i32, i32);

/// Triple identifying *which* snapshot a measure call should match
/// against — the inputs the cache keys on. Built once at the top of a
/// measure call and reused at the cache write-back at the bottom, so
/// `WidgetId` / hash / quantization are looked up only once per node.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SubtreeCacheKey {
    pub wid: WidgetId,
    pub subtree_hash: NodeHash,
    pub available_q: AvailableKey,
}

#[inline]
fn quantize_axis(v: f32) -> i32 {
    if !v.is_finite() {
        i32::MAX
    } else {
        v.round() as i32
    }
}

#[inline]
pub(crate) fn quantize_available(s: Size) -> AvailableKey {
    (quantize_axis(s.w), quantize_axis(s.h))
}

#[derive(Default)]
pub(crate) struct MeasureCache {
    /// Per-`WidgetId` snapshot from the previous frame's measure pass.
    /// Mutated in place: a hit reads the entry, a miss overwrites it
    /// (or inserts a new one) at the end of measure. Inner `Vec`
    /// capacities are retained across frames.
    pub prev: FxHashMap<WidgetId, SubtreeSnapshot>,
}

impl MeasureCache {
    /// Drop snapshots for widgets that were present last frame but not
    /// this one. Called once per frame from `Ui::end_frame`, fed the
    /// same `removed` slice that `Damage` and `TextMeasurer` consume.
    pub fn sweep_removed(&mut self, removed: &[WidgetId]) {
        for wid in removed {
            self.prev.remove(wid);
        }
    }

    /// Overwrite (or insert) `wid`'s snapshot from disjoint slices on
    /// `LayoutEngine`. Splitting the write into a method on the cache
    /// lets the caller hold immutable borrows of `LayoutEngine.desired`
    /// and `LayoutResult.text_shapes` while we mutate `cache.prev` —
    /// the borrows are field-disjoint.
    pub fn write_subtree(
        &mut self,
        key: SubtreeCacheKey,
        desired: &[Size],
        text_shapes: &[Option<ShapedText>],
    ) {
        debug_assert_eq!(desired.len(), text_shapes.len());
        let snap = self.prev.entry(key.wid).or_default();
        snap.subtree_hash = key.subtree_hash;
        snap.available_q = key.available_q;
        snap.desired.clear();
        snap.desired.extend_from_slice(desired);
        snap.text_shapes.clear();
        snap.text_shapes.extend_from_slice(text_shapes);
    }
}

#[cfg(test)]
mod tests;
