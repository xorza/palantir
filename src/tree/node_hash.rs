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

#[cfg(test)]
mod tests {
    use crate::common::hash::Hasher;
    use crate::layout::types::align::Align;
    use crate::primitives::color::Color;
    use crate::shape::{Shape, TextWrap};
    use std::borrow::Cow;
    use std::hash::{Hash, Hasher as _};

    fn text_shape(font_size_px: f32, line_height_px: f32) -> Shape {
        Shape::Text {
            local_rect: None,
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
        text_shape(16.0, 16.0 * 1.2).hash(&mut h_a);
        let a = h_a.finish();
        let mut h_b = Hasher::new();
        text_shape(16.0, 16.0 * 1.5).hash(&mut h_b);
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
        text_shape(16.0, 19.2).hash(&mut h_a);
        let mut h_b = Hasher::new();
        text_shape(16.0, 19.2).hash(&mut h_b);
        assert_eq!(h_a.finish(), h_b.finish());
    }
}
