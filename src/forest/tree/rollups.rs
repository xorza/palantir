//! Per-node and per-subtree authoring-hash columns populated when a
//! recorded tree is finalized.

use crate::common::content_hash::ContentHash;

/// Per-node hash columns populated by [`crate::forest::Tree::post_record`].
/// Both slices index by `NodeId.0` and are length `records.len()`
/// after `post_record`. Capacity retained across frames.
///
/// - `node[i]` — authoring hash of node `i` alone (layout / paint /
///   extras / shapes / grid def). Read by damage diff and the leaf
///   intrinsic cache.
/// - `subtree[i]` — rollup of `node[i]` together with the subtree
///   hashes of `i`'s direct children, in declaration order. Equality
///   across frames means nothing in the subtree changed; the
///   cross-frame measure cache keys on this. See
///   `src/layout/measure-cache.md`.
///
/// Per-chrome authoring hash lives inline on `ChromeRow.hash` (only
/// chromed nodes pay storage); per-shape canonical hash lives on
/// `Tree.shapes.hashes`.
///
/// "Does this node directly contribute pixels?" used to live here as
/// a `paints: FixedBitSet`; the unified
/// `Cascades::paint_arenas[].node_spans` answers a superset of that
/// question (empty span means "no rows" — no chrome, no shapes, and
/// no children, since child markers occupy rows too), so the bitset
/// was removed.
#[derive(Debug, Default)]
pub(crate) struct SubtreeRollups {
    pub(crate) node: Vec<ContentHash>,
    pub(crate) subtree: Vec<ContentHash>,
}

impl SubtreeRollups {
    /// Reset and size every column for `n` records. Columns are
    /// resized with default values — filled by indexed assignment
    /// during the fused reverse-pre-order pass in
    /// `Tree::compute_hashes`.
    pub(crate) fn reset_for(&mut self, n: usize) {
        // Single-pass resize: `compute_hashes` overwrites every slot
        // via indexed assignment, so the fill value is irrelevant —
        // `resize` is preferred over `clear()+resize_with` because it
        // avoids the truncate-then-grow round trip when `n` is steady.
        self.node.resize(n, ContentHash::default());
        self.subtree.resize(n, ContentHash::default());
    }
}

#[cfg(test)]
mod tests {
    use crate::common::hash::hash_str;
    use crate::forest::shapes::hash::compute_record_hash;
    use crate::forest::shapes::record::ShapeRecord;
    use crate::layout::types::align::Align;
    use crate::primitives::color::Color;
    use crate::primitives::interned_str::InternedStr;
    use crate::shape::TextWrap;
    use crate::text::{FontFamily, FontWeight};

    fn text_shape(
        line_height_px: f32,
        weight: FontWeight,
        local_origin: Option<glam::Vec2>,
    ) -> ShapeRecord {
        ShapeRecord::Text {
            local_origin,
            text: InternedStr::from("hi"),
            text_hash: hash_str("hi"),
            color: Color::WHITE.into(),
            font_size_px: 16.0,
            line_height_px,
            wrap: TextWrap::Truncate,
            align: Align::default(),
            family: FontFamily::Sans,
            weight,
        }
    }

    fn hash_shape(s: &ShapeRecord) -> u64 {
        compute_record_hash(s).0
    }

    /// Pin: every authoring-relevant `ShapeRecord::Text` field participates
    /// in the node hash. Without this, the measure cache would
    /// conflate runs whose shaped buffers genuinely differ
    /// (`line_height_px` → different `Metrics::new`; `weight` → different
    /// physical face) or whose paint position differs (`local_rect` →
    /// different `DrawText` rects). New fields go in the table, not in a
    /// new test.
    #[test]
    fn text_shape_hash_distinguishes_each_authoring_field() {
        use FontWeight::{Bold, Regular};
        let o_a = Some(glam::Vec2::new(0.0, 0.0));
        let o_b = Some(glam::Vec2::new(5.0, 5.0));
        let cases: [(&str, ShapeRecord, ShapeRecord); 4] = [
            (
                "line_height_px",
                text_shape(16.0 * 1.2, Regular, None),
                text_shape(16.0 * 1.5, Regular, None),
            ),
            (
                "weight Regular vs Bold",
                text_shape(19.2, Regular, None),
                text_shape(19.2, Bold, None),
            ),
            (
                "local_origin None vs Some",
                text_shape(19.2, Regular, None),
                text_shape(19.2, Regular, o_a),
            ),
            (
                "local_origin Some(a) vs Some(b)",
                text_shape(19.2, Regular, o_a),
                text_shape(19.2, Regular, o_b),
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
            hash_shape(&text_shape(19.2, FontWeight::Regular, None)),
            hash_shape(&text_shape(19.2, FontWeight::Regular, None)),
        );
    }
}
