//! Cross-frame measure cache (Phase 1: leaf-only). Skip a `Leaf`
//! node's measure body when its `subtree_hash` and incoming
//! `available` size both match last frame. See `docs/measure-cache.md`.
//!
//! Keyed by stable [`WidgetId`]; lifecycle mirrors [`crate::ui::Damage`]
//! and [`crate::text::TextMeasurer`] — eviction piggy-backs on the
//! `SeenIds.removed()` diff at frame end.

use crate::layout::result::ShapedText;
use crate::primitives::{Size, WidgetId};
use crate::tree::NodeHash;
use rustc_hash::FxHashMap;

/// Snapshot of one leaf node's measure output. Stored across frames
/// so a cache hit can replay `desired` (and any text shape) without
/// re-running the leaf body.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LeafSnapshot {
    /// Subtree hash from last frame. For a leaf the subtree hash is a
    /// deterministic function of the node hash; we still store the
    /// rolled value to keep this struct shape ready for Phase 2's
    /// subtree-skip extension.
    pub subtree_hash: NodeHash,
    /// `available` size the leaf was measured against last frame,
    /// quantized to integer logical pixels. Sub-pixel jitter from a
    /// resizing parent shouldn't bust the cache.
    pub available_q: AvailableKey,
    /// Outer (margin-inclusive) size returned by `measure`.
    pub desired: Size,
    /// `Some` for `Shape::Text` leaves — the cosmic shape result that
    /// landed on `LayoutResult.text_shapes` during the original
    /// measure. `None` for leaves with no text shape.
    pub text_shape: Option<ShapedText>,
}

/// Quantized (`available.w`, `available.h`) used as the cache's
/// dimensional key. `i32::MAX` represents an infinite axis (ZStack /
/// Hug parents propagate `f32::INFINITY`).
pub(crate) type AvailableKey = (i32, i32);

/// Triple identifying *which* snapshot a leaf measure should match
/// against — the inputs the cache keys on. Built once at the top of a
/// leaf measure and reused at the cache write-back at the bottom, so
/// `WidgetId` / hash / quantization are looked up only once per node.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LeafCacheKey {
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
    /// at the end of measure. Capacity retained across frames.
    pub prev: FxHashMap<WidgetId, LeafSnapshot>,
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
}

#[cfg(test)]
mod tests;
