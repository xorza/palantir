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

use super::{GridArena, NodeHashes, NodeId, NodeRecord, TreeOp};
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct NodeHash(u64);

impl NodeHash {
    /// Sentinel returned by `Tree::node_hash` before
    /// `compute_hashes` runs. Distinguishable from any real hash only
    /// probabilistically (collisions are 2⁻⁶⁴), but adequate as an
    /// "uninitialized" marker.
    pub(crate) const UNCOMPUTED: Self = Self(0);

    /// Construct a `NodeHash` from a raw `u64`. Used by the rollup
    /// loop and by tests that synthesize sentinel hashes.
    #[inline]
    pub(crate) fn from_u64(v: u64) -> Self {
        Self(v)
    }
}

impl NodeHashes {
    /// Per-frame entry point called by `Tree::end_frame`: populates
    /// `node[i]`, `subtree[i]`, and `subtree_has_grid`. Field-borrow
    /// signature instead of `&Tree` so the caller can split-borrow
    /// `&mut self.hashes` from the read-only tree columns.
    pub(crate) fn compute(
        &mut self,
        records: &Soa<NodeRecord>,
        extras: &SparseColumn<ElementExtras>,
        chrome: &SparseColumn<Background>,
        kinds: &[TreeOp],
        shapes: &[Shape],
        grid: &GridArena,
    ) {
        self.compute_per_node(records, extras, chrome, kinds, shapes, grid);
        self.compute_subtree_rollup(records, extras);
    }

    /// Phase 1: compute every node's authoring hash in a single
    /// pre-order pass over `kinds`. O(N) total work.
    fn compute_per_node(
        &mut self,
        records: &Soa<NodeRecord>,
        extras: &SparseColumn<ElementExtras>,
        chrome: &SparseColumn<Background>,
        kinds: &[TreeOp],
        shapes: &[Shape],
        grid: &GridArena,
    ) {
        self.node.clear();
        self.node.resize(records.len(), NodeHash::UNCOMPUTED);
        self.compute_stack.clear();

        let stack = &mut self.compute_stack;
        let out = &mut self.node;
        let mut next_id: u32 = 0;
        let mut shape_cursor: usize = 0;

        for op in kinds {
            match op {
                TreeOp::NodeEnter => {
                    if let Some((_, parent_h)) = stack.last_mut() {
                        parent_h.write_u8(0xFF);
                    }
                    let id = NodeId(next_id);
                    next_id += 1;
                    let i = id.index();
                    let mut h = Hasher::new();
                    hash_layout_core(&mut h, &records.layout()[i], records.attrs()[i]);
                    if let Some(e) = extras.get(i) {
                        hash_node_extras(&mut h, e);
                    }
                    hash_chrome(&mut h, chrome.get(i));
                    stack.push((id, h));
                }
                TreeOp::Shape => {
                    let (_, h) = stack
                        .last_mut()
                        .expect("Shape op outside any open NodeEnter");
                    hash_shape(h, &shapes[shape_cursor]);
                    shape_cursor += 1;
                }
                TreeOp::NodeExit => {
                    let (id, mut h) = stack.pop().expect("NodeExit op without matching NodeEnter");
                    if let LayoutMode::Grid(idx) = records.layout()[id.index()].mode {
                        hash_grid_def(&mut h, &grid.defs[idx as usize]);
                    }
                    out[id.index()] = NodeHash::from_u64(h.finish());
                }
            }
        }
        debug_assert!(
            stack.is_empty(),
            "kinds stream ended with {} open hashers",
            stack.len(),
        );
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
        self.subtree.resize(n, NodeHash::UNCOMPUTED);
        self.subtree_has_grid.clear();
        self.subtree_has_grid.grow(n);
        for i in (0..n).rev() {
            let end = records.end()[i];
            let mut h = Hasher::new();
            self.node[i].hash(&mut h);
            if let Some(t) = extras.get(i).and_then(|e| e.transform) {
                h.write_u8(1);
                h.pod(&t);
            } else {
                h.write_u8(0);
            }
            let mut has_grid = matches!(records.layout()[i].mode, LayoutMode::Grid(_));
            let mut next = (i as u32) + 1;
            while next < end {
                self.subtree[next as usize].hash(&mut h);
                has_grid |= self.subtree_has_grid.contains(next as usize);
                next = records.end()[next as usize];
            }
            self.subtree[i] = NodeHash::from_u64(h.finish());
            self.subtree_has_grid.set(i, has_grid);
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
    h.write_u16(((l.visibility as u8 as u16) << 8) | l.align.raw() as u16);
    // sense(3 bits @0) | disabled(@8) | clip(2 bits @9) | focusable(@11).
    let packed = (attrs.sense() as u16)
        | ((attrs.is_disabled() as u16) << 8)
        | ((attrs.clip_mode() as u16) << 9)
        | ((attrs.is_focusable() as u16) << 11);
    h.write_u16(packed);
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
    h.write_u16(((e.child_align.raw() as u16) << 8) | e.justify as u8 as u16);
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
