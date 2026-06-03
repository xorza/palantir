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
//!
//! # Producer catalogue
//!
//! Every authoring hash that feeds a cross-frame cache is produced
//! once, where its inputs become immutable. Consumers read precomputed
//! `u64`s rather than re-walking fields. Sites:
//!
//! | What                                  | Producer site                                   | Consumer                                |
//! | ------------------------------------- | ----------------------------------------------- | --------------------------------------- |
//! | Per-shape canonical hash              | `forest::shapes::Shapes::add`                   | `Tree::compute_hashes`, damage diff     |
//! | Per-chrome canonical hash             | `forest::frame_arena::FrameArena::lower_background` | `Tree::compute_hashes`, damage diff |
//! | Per-text bytes hash                   | `forest::frame_arena::FrameArena::hash_text`    | `ShapeRecord::Text.text_hash`           |
//! | Per-gradient content hash             | `forest::frame_arena::grad_hash`                | `ShapeRecord::*.fill_grad_hash`         |
//! | Per-polyline content hash             | `forest::frame_arena::FrameArenaInner::lower_polyline_inner` | `ShapeRecord::Polyline.content_hash` |
//! | Per-bezier content hash               | `forest::frame_arena::lower_curve_inner`        | `ShapeRecord::Curve.content_hash`       |
//! | Per-mesh content hash                 | `primitives::mesh::Mesh::content_hash`          | `ShapeRecord::Mesh.content_hash`        |
//! | Per-node + per-subtree rollup         | `forest::tree::Tree::compute_hashes`            | damage diff, measure cache              |
//! | Per-cascade input hash                | `ui::cascade::hash_cascade_input`               | damage subtree-skip predicate           |
//!
//! Adding an authoring field that should invalidate caches: pick the
//! producer above whose output the field belongs to, and extend its
//! fold. The downstream `Hash` impls (`ChromeRow`, `ShapeRecord`,
//! `BoundsExtras`, `PanelExtras`, `LayoutCore::hash_with_flags`,
//! `GridDef`) only walk fields that aren't already covered by a
//! pre-baked sub-hash.

/// Authoring-hash newtype. A 64-bit `FxHash` over the inputs that
/// affect rendering output for one node — *not* the derived layout
/// output. Wrapping `u64` rather than passing it bare prevents
/// confusion with `WidgetId` / other 64-bit handles in signatures
/// like `shape_unbounded(wid: WidgetId, hash: NodeHash, …)`.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct NodeHash(pub(crate) u64);

/// Per-node fingerprint of cascade inputs flowing in from ancestors
/// (parent transform/clip/disabled/invisible) plus the node's own
/// arranged rect, packed with the resolved `invisible` bit. Folded
/// into a 64-bit `FxHash` (lower 63 bits) during the cascade walk;
/// the high bit holds the cascade-resolved `invisible` so encoder
/// and damage can read both in one 8-byte load. Compared
/// frame-over-frame by `DamageEngine::compute`: if this matches AND
/// `subtree[i]` matches, the entire subtree's paint state is
/// bit-identical by induction and the per-node diff jumps to
/// `subtree_end[i]`.
///
/// Why packing is sound: the skip predicate also requires
/// `subtree[i]` match, which covers every descendant's `node_hash`
/// (where own visibility lives). If `subtree` matches AND the lower
/// 63 hash bits match, the high `invisible` bit is implied — own
/// visibility is in `node_hash`, parent_invisible is in the hash
/// inputs.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CascadeInputHash(pub(crate) u64);

const INVISIBLE_BIT: u64 = 1u64 << 63;
const HASH_MASK: u64 = !INVISIBLE_BIT;

impl CascadeInputHash {
    /// Combine a raw 64-bit hash output with the cascade-resolved
    /// `invisible` flag. The hash's top bit is masked off before the
    /// flag is shifted into place — 63 bits of entropy is more than
    /// enough for the skip predicate, and branchless avoids the cost
    /// of a per-node conditional move on the hot cascade path.
    #[inline]
    pub(crate) fn pack(hash: u64, invisible: bool) -> Self {
        Self((hash & HASH_MASK) | ((invisible as u64) << 63))
    }

    #[inline]
    pub(crate) fn invisible(self) -> bool {
        self.0 & INVISIBLE_BIT != 0
    }
}

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
/// `Cascades::paint_arenas[].node_spans` answers the same question
/// (empty span means "paints nothing"), so the bitset was removed.
#[derive(Default)]
pub(crate) struct SubtreeRollups {
    pub(crate) node: Vec<NodeHash>,
    pub(crate) subtree: Vec<NodeHash>,
}

impl SubtreeRollups {
    /// Reset and size every column for `n` records. Both columns are
    /// resized with default values — filled by indexed assignment
    /// during the fused reverse-pre-order pass in
    /// `Tree::compute_hashes`.
    pub(crate) fn reset_for(&mut self, n: usize) {
        // Single-pass resize: `compute_hashes` overwrites every slot
        // via indexed assignment, so the fill value is irrelevant —
        // `resize` is preferred over `clear()+resize_with` because it
        // avoids the truncate-then-grow round trip when `n` is steady.
        self.node.resize(n, NodeHash::default());
        self.subtree.resize(n, NodeHash::default());
    }
}

#[cfg(test)]
mod tests {
    use crate::forest::frame_arena::FrameArena;
    use crate::forest::shapes::record::ShapeRecord;
    use crate::layout::types::align::Align;
    use crate::primitives::color::Color;
    use crate::primitives::interned_str::InternedStr;
    use crate::shape::TextWrap;

    fn text_shape(line_height_px: f32, local_origin: Option<glam::Vec2>) -> ShapeRecord {
        ShapeRecord::Text {
            local_origin,
            text: InternedStr::from("hi"),
            text_hash: FrameArena::hash_text("hi"),
            color: Color::WHITE.into(),
            font_size_px: 16.0,
            line_height_px,
            wrap: TextWrap::Truncate,
            align: Align::default(),
            family: crate::text::FontFamily::Sans,
        }
    }

    fn hash_shape(s: &ShapeRecord) -> u64 {
        crate::forest::shapes::hash::compute_record_hash(s).0
    }

    /// Pin: every authoring-relevant `ShapeRecord::Text` field participates
    /// in the node hash. Without this, the measure cache would
    /// conflate runs whose shaped buffers genuinely differ
    /// (`line_height_px` → different `Metrics::new`) or whose paint
    /// position differs (`local_rect` → different `DrawText` rects).
    /// New fields go in the table, not in a new test.
    #[test]
    fn text_shape_hash_distinguishes_each_authoring_field() {
        let o_a = Some(glam::Vec2::new(0.0, 0.0));
        let o_b = Some(glam::Vec2::new(5.0, 5.0));
        let cases: [(&str, ShapeRecord, ShapeRecord); 3] = [
            (
                "line_height_px",
                text_shape(16.0 * 1.2, None),
                text_shape(16.0 * 1.5, None),
            ),
            (
                "local_origin None vs Some",
                text_shape(19.2, None),
                text_shape(19.2, o_a),
            ),
            (
                "local_origin Some(a) vs Some(b)",
                text_shape(19.2, o_a),
                text_shape(19.2, o_b),
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
