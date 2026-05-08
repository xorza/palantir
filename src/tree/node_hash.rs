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

/// Authoring-hash newtype. A 64-bit `FxHash` over the inputs that
/// affect rendering output for one node — *not* the derived layout
/// output. Wrapping `u64` rather than passing it bare prevents
/// confusion with `WidgetId` / other 64-bit handles in signatures
/// like `shape_unbounded(wid: WidgetId, hash: NodeHash, …)`.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct NodeHash(pub(crate) u64);

/// Subtree-wide rollup data populated by [`super::Tree::end_frame`].
/// All three slices/sets index by `NodeId.0` and are length
/// `records.len()` after `end_frame`. Capacity retained across frames.
///
/// - `node[i]` — authoring hash of node `i` alone (layout / paint /
///   extras / shapes / grid def). Read by damage diff and the leaf
///   intrinsic cache.
/// - `subtree[i]` — rollup of `node[i]` together with the subtree
///   hashes of `i`'s direct children, in declaration order. Equality
///   across frames means nothing in the subtree changed; the
///   cross-frame measure cache keys on this. See
///   `src/layout/measure-cache.md`.
/// - `has_grid[i]` — bit `i` is true iff the subtree rooted at node
///   `i` contains any `LayoutMode::Grid` node. Fast-path skip for
///   `MeasureCache`'s grid-hug snapshot/restore walk. Conceptually a
///   structure summary, not a hash, but bundled here because it has
///   the same lifecycle as the hash columns (populated by `end_frame`,
///   indexed by `NodeId`, read by the same caches).
#[derive(Default)]
pub(crate) struct SubtreeRollups {
    pub(crate) node: Vec<NodeHash>,
    pub(crate) subtree: Vec<NodeHash>,
    pub(crate) has_grid: fixedbitset::FixedBitSet,
}

impl SubtreeRollups {
    /// Reset the *hash* columns and size them for `n` records. `node`
    /// is cleared with reserved capacity (filled by appending during
    /// `compute_node_hashes`); `subtree` is cleared and resized with
    /// default values (written by indexed assignment in
    /// `compute_subtree_hashes`'s reverse pre-order walk). `has_grid`
    /// is *not* touched here — its lifecycle is owned by recording
    /// (cleared at `begin_frame`, populated by `open_node`/`close_node`,
    /// permuted by `reorder_records`).
    pub(crate) fn reset_hashes_for(&mut self, n: usize) {
        self.node.clear();
        self.node.reserve(n);
        self.subtree.clear();
        self.subtree.resize_with(n, NodeHash::default);
    }
}

#[cfg(test)]
mod tests {
    use crate::common::hash::Hasher;
    use crate::layout::types::align::Align;
    use crate::primitives::color::Color;
    use crate::primitives::rect::Rect;
    use crate::shape::{Shape, TextWrap};
    use std::borrow::Cow;
    use std::hash::{Hash, Hasher as _};

    fn text_shape(line_height_px: f32, local_rect: Option<Rect>) -> Shape {
        Shape::Text {
            local_rect,
            text: Cow::Borrowed("hi"),
            color: Color::WHITE,
            font_size_px: 16.0,
            line_height_px,
            wrap: TextWrap::Single,
            align: Align::default(),
        }
    }

    fn hash_shape(s: &Shape) -> u64 {
        let mut h = Hasher::new();
        s.hash(&mut h);
        h.finish()
    }

    /// Pin: every authoring-relevant `Shape::Text` field participates
    /// in the node hash. Without this, the measure cache would
    /// conflate runs whose shaped buffers genuinely differ
    /// (`line_height_px` → different `Metrics::new`) or whose paint
    /// position differs (`local_rect` → different `DrawText` rects).
    /// New fields go in the table, not in a new test.
    #[test]
    fn text_shape_hash_distinguishes_each_authoring_field() {
        let r_a = Some(Rect::new(0.0, 0.0, 10.0, 10.0));
        let r_b = Some(Rect::new(5.0, 5.0, 10.0, 10.0));
        let cases: [(&str, Shape, Shape); 3] = [
            (
                "line_height_px",
                text_shape(16.0 * 1.2, None),
                text_shape(16.0 * 1.5, None),
            ),
            (
                "local_rect None vs Some",
                text_shape(19.2, None),
                text_shape(19.2, r_a),
            ),
            (
                "local_rect Some(a) vs Some(b)",
                text_shape(19.2, r_a),
                text_shape(19.2, r_b),
            ),
        ];
        for (label, a, b) in cases {
            assert_ne!(
                hash_shape(&a),
                hash_shape(&b),
                "case `{label}`: distinct fields must hash differently",
            );
        }
    }

    /// Sanity counterpart: identical shapes hash identically (guards
    /// against accidental non-determinism, e.g. a future field
    /// hashed via a `RandomState` or rand call).
    #[test]
    fn text_shape_hash_matches_when_inputs_match() {
        assert_eq!(
            hash_shape(&text_shape(19.2, None)),
            hash_shape(&text_shape(19.2, None)),
        );
    }
}
