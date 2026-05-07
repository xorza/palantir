//! Per-node authoring-hash computation. Walks every field that affects
//! rendering output and folds it into a 64-bit `FxHash`. Captures the
//! "what the user typed" snapshot for a node — the inputs, not the
//! derived layout output (`rect`, `desired`).
//!
//! Feeds the damage pass in `src/ui/damage/`: each frame's hash is
//! diffed against the prev-frame snapshot keyed by `WidgetId`.
//!
//! All `f32` fields hash via `to_bits()` — exact bit equality, not
//! `==`-equality, so `0.0` vs `-0.0` hash differently (over-eager dirty
//! marking, fine for our use). NaN handling is consistent for the same
//! NaN bit pattern; UI authoring shouldn't produce NaN anyway (asserts
//! in builders enforce non-negative sizes etc.).

use super::{GridArena, NodeId, NodeRecord, TreeItem, iter_direct_items};
use crate::common::hash::Hasher;
use crate::common::sparse_column::SparseColumn;
use crate::layout::types::{sizing::Sizes, sizing::Sizing, track::Track};
use crate::primitives::background::Background;
use crate::shape::Shape;
use crate::tree::element::{ElementExtras, LayoutCore, LayoutMode, PaintAttrs, ScrollAxes};
use crate::widgets::grid::GridDef;
use soa_rs::Soa;

use std::hash::Hash;
use std::hash::Hasher as _;

/// Authoring-hash newtype. A 64-bit `FxHash` over the inputs that
/// affect rendering output for one node — *not* the derived layout
/// output. Wrapping `u64` rather than passing it bare prevents
/// confusion with `WidgetId` / other 64-bit handles in signatures
/// like `shape_unbounded(wid: WidgetId, hash: NodeHash, …)`.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct NodeHash(pub(crate) u64);

/// Per-node hash data populated by [`super::Tree::end_frame`].
///
/// - `node[i]` — authoring hash of node `i` alone (layout / paint /
///   extras / shapes / grid def). Read by damage diff and the leaf
///   intrinsic cache.
/// - `subtree[i]` — rollup of `node[i]` together with the subtree
///   hashes of `i`'s direct children, in declaration order. Equality
///   across frames means nothing in the subtree changed; the cross-frame
///   measure cache and encode cache both key on this. See
///   `src/layout/measure-cache.md` and
///   `src/renderer/frontend/encoder/encode-cache.md`.
///
/// Both vecs are length `records.len()` after `end_frame`. Capacity
/// retained across frames.
#[derive(Default)]
pub(crate) struct NodeHashes {
    pub(crate) node: Vec<NodeHash>,
    pub(crate) subtree: Vec<NodeHash>,
}

impl NodeHashes {
    /// Per-frame entry point called by `Tree::end_frame`: populates
    /// `node[i]` and `subtree[i]`. Field-borrow signature instead of
    /// `&Tree` so the caller can split-borrow `&mut self.hashes` from
    /// the read-only tree columns.
    pub(crate) fn compute(
        &mut self,
        records: &Soa<NodeRecord>,
        extras: &SparseColumn<ElementExtras>,
        chrome: &SparseColumn<Background>,
        shapes: &[Shape],
        grid: &GridArena,
    ) {
        self.compute_per_node(records, extras, chrome, shapes, grid);
        self.compute_subtree_rollup(records, extras);
    }

    /// Phase 1: per-node authoring hash. For each node, hash its layout /
    /// extras / chrome, then walk `iter_direct_items` and feed each
    /// direct shape into the hasher with a `0xFF` marker between
    /// children — the marker preserves "shape vs child boundary"
    /// ordering across siblings.
    fn compute_per_node(
        &mut self,
        records: &Soa<NodeRecord>,
        extras: &SparseColumn<ElementExtras>,
        chrome: &SparseColumn<Background>,
        shapes: &[Shape],
        grid: &GridArena,
    ) {
        let n = records.len();
        self.node.clear();
        self.node.reserve(n);

        let layouts = records.layout();
        let attrs_col = records.attrs();

        for i in 0..n {
            let mut h = Hasher::new();
            hash_layout_core(&mut h, &layouts[i], attrs_col[i]);
            if let Some(e) = extras.get(i) {
                hash_node_extras(&mut h, e);
            }
            hash_chrome(&mut h, chrome.get(i));

            for item in iter_direct_items(records, shapes, NodeId(i as u32)) {
                match item {
                    TreeItem::Shape(s) => hash_shape(&mut h, s),
                    TreeItem::Child(_) => h.write_u8(0xFF),
                }
            }

            if let LayoutMode::Grid(idx) = layouts[i].mode {
                hash_grid_def(&mut h, &grid.defs[idx as usize]);
            }
            self.node.push(NodeHash(h.finish()));
        }
    }

    /// Phase 2: subtree-hash rollup. Reverse pre-order so children fill
    /// before parents read. `transform` folds in here (not `node[i]`) so
    /// the encode cache invalidates on transform-only changes while
    /// damage rect-diffing keeps owning paint-position drift.
    fn compute_subtree_rollup(
        &mut self,
        records: &Soa<NodeRecord>,
        extras: &SparseColumn<ElementExtras>,
    ) {
        let n = records.len();
        self.subtree.clear();
        self.subtree.resize_with(n, NodeHash::default);

        let ends = records.end();

        for i in (0..n).rev() {
            let end = ends[i];
            let mut h = Hasher::new();
            h.write_u64(self.node[i].0);
            if let Some(t) = extras.get(i).and_then(|e| e.transform) {
                h.write_u8(1);
                h.pod(&t);
            } else {
                h.write_u8(0);
            }
            let mut next = (i as u32) + 1;
            while next < end {
                h.write_u64(self.subtree[next as usize].0);
                next = ends[next as usize];
            }
            self.subtree[i] = NodeHash(h.finish());
        }
    }
}

/// `Sizing` is a tagged union with niche-uninit padding in its inactive
/// variant — `pod` would hash junk bytes. Encode as a deterministic
/// `tag:u8 + value:f32` instead. Inlined for the two `Sizes` axes.
#[inline]
fn hash_sizing(h: &mut Hasher, s: Sizing) {
    let (tag, v) = match s {
        Sizing::Fixed(v) => (0u8, v),
        Sizing::Hug => (1, 0.0),
        Sizing::Fill(w) => (2, w),
    };
    h.write_u8(tag);
    h.write_u32(v.to_bits());
}

#[inline]
fn hash_sizes(h: &mut Hasher, s: Sizes) {
    hash_sizing(h, s.w);
    hash_sizing(h, s.h);
}

/// `Grid(idx)` collapses to the same tag as the other variants — `idx`
/// is a frame-local arena slot that shifts with sibling order, while the
/// def's actual content is hashed at `NodeExit` via `hash_grid_def`.
#[inline]
fn hash_layout_mode(h: &mut Hasher, m: LayoutMode) {
    let tag: u8 = match m {
        LayoutMode::Leaf => 0,
        LayoutMode::HStack => 1,
        LayoutMode::VStack => 2,
        LayoutMode::WrapHStack => 3,
        LayoutMode::WrapVStack => 4,
        LayoutMode::ZStack => 5,
        LayoutMode::Canvas => 6,
        LayoutMode::Grid(_) => 7,
        LayoutMode::Scroll(ScrollAxes::Vertical) => 8,
        LayoutMode::Scroll(ScrollAxes::Horizontal) => 9,
        LayoutMode::Scroll(ScrollAxes::Both) => 10,
    };
    h.write_u8(tag);
}

#[inline]
fn hash_layout_core(h: &mut Hasher, l: &LayoutCore, attrs: PaintAttrs) {
    hash_layout_mode(h, l.mode);
    hash_sizes(h, l.size);
    h.pod(&[l.padding, l.margin]);
    h.write_u8(l.visibility as u8);
    h.write_u8(l.align.raw());
    h.write_u8(attrs.bits);
}

#[inline]
fn hash_node_extras(h: &mut Hasher, e: &ElementExtras) {
    // `transform` is intentionally omitted: it doesn't affect this
    // node's own paint (the encoder draws the node at its layout rect
    // *before* `PushTransform`; the transform composes into
    // descendants' screen rects via `Cascades`). A parent transform
    // change shows up as descendant screen-rect diffs in
    // `Damage::compute`, which is the right granularity.
    //
    // Transform IS folded into `subtree_hash` separately (in the tree's
    // rollup loop) so the encode cache — which replays cached command
    // buffers with the original `PushTransform` baked in — invalidates
    // on transform-only changes.
    h.pod(&e.position);
    h.pod(&e.grid);
    h.pod(&[e.min_size, e.max_size]);
    h.pod(&[e.gap, e.line_gap]);
    h.write_u8(e.child_align.raw());
    h.write_u8(e.justify as u8);
}

#[inline]
fn hash_chrome(h: &mut Hasher, chrome: Option<&Background>) {
    match chrome {
        None => h.write_u8(0),
        Some(bg) => {
            h.write_u8(1);
            h.pod(&bg.fill);
            h.pod(&bg.radius);
            match bg.stroke {
                None => h.write_u8(0),
                Some(s) => {
                    h.write_u8(1);
                    h.pod(&s);
                }
            }
        }
    }
}

#[inline]
fn hash_shape(h: &mut Hasher, shape: &Shape) {
    match shape {
        Shape::RoundedRect {
            radius,
            fill,
            stroke,
        } => {
            h.write_u8(0);
            h.pod(radius);
            h.pod(fill);
            match stroke {
                None => h.write_u8(0),
                Some(s) => {
                    h.write_u8(1);
                    h.pod(s);
                }
            }
        }
        Shape::SubRect {
            local_rect,
            radius,
            fill,
            stroke,
        } => {
            h.write_u8(3);
            h.pod(local_rect);
            h.pod(radius);
            h.pod(fill);
            match stroke {
                None => h.write_u8(0),
                Some(s) => {
                    h.write_u8(1);
                    h.pod(s);
                }
            }
        }
        Shape::Line { a, b, width, color } => {
            h.write_u8(1);
            h.pod(a);
            h.pod(b);
            h.write_u32(width.to_bits());
            h.pod(color);
        }
        Shape::Text {
            text,
            color,
            font_size_px,
            line_height_px,
            wrap,
            align,
        } => {
            h.write_u8(2);
            text.hash(h);
            h.pod(color);
            h.write_u32(font_size_px.to_bits());
            h.write_u32(line_height_px.to_bits());
            h.write_u16(((align.raw() as u16) << 8) | *wrap as u8 as u16);
        }
    }
}

#[inline]
fn hash_track(h: &mut Hasher, t: &Track) {
    hash_sizing(h, t.size);
    h.write_u32(t.min.to_bits());
    h.write_u32(t.max.to_bits());
}

#[inline]
fn hash_grid_def(h: &mut Hasher, def: &GridDef) {
    h.write_u32(def.rows.len() as u32);
    for t in def.rows.iter() {
        hash_track(h, t);
    }
    h.write_u32(def.cols.len() as u32);
    for t in def.cols.iter() {
        hash_track(h, t);
    }
    h.write_u32(def.row_gap.to_bits());
    h.write_u32(def.col_gap.to_bits());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::types::align::Align;
    use crate::primitives::color::Color;
    use crate::shape::TextWrap;
    use std::borrow::Cow;

    fn text_shape(font_size_px: f32, line_height_px: f32) -> Shape {
        Shape::Text {
            text: Cow::Borrowed("hi"),
            color: Color::WHITE,
            font_size_px,
            line_height_px,
            wrap: TextWrap::Single,
            align: Align::default(),
        }
    }

    #[test]
    fn text_shape_hash_differs_when_line_height_differs() {
        // Pin: two `Shape::Text` runs that differ only in
        // `line_height_px` must hash differently. Without this the
        // measure cache would conflate runs whose shaped buffers
        // genuinely differ (different `Metrics::new`).
        let mut h_a = Hasher::new();
        hash_shape(&mut h_a, &text_shape(16.0, 16.0 * 1.2));
        let a = h_a.finish();
        let mut h_b = Hasher::new();
        hash_shape(&mut h_b, &text_shape(16.0, 16.0 * 1.5));
        let b = h_b.finish();
        assert_ne!(
            a, b,
            "different line_height_px must produce different node hashes",
        );
    }

    #[test]
    fn text_shape_hash_matches_when_line_height_matches() {
        // Sanity counterpart: identical shapes hash identically (no
        // accidental introduction of non-determinism via the new field).
        let mut h_a = Hasher::new();
        hash_shape(&mut h_a, &text_shape(16.0, 19.2));
        let mut h_b = Hasher::new();
        hash_shape(&mut h_b, &text_shape(16.0, 19.2));
        assert_eq!(h_a.finish(), h_b.finish());
    }
}
