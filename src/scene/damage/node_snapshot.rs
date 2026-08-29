//! What the damage diff remembers about one widget between frames.

use crate::common::content_hash::ContentHash;
use crate::primitives::span::Span;
use crate::scene::cascade::CascadeInputHash;

/// Per-widget snapshot held in [`crate::scene::damage::DamageEngine::prev`], keyed by stable
/// [`WidgetId`](crate::primitives::widget_id::WidgetId). Only widgets with
/// paint rows last frame have an entry
/// — rowless nodes (e.g. a popup's childless invisible click-eater)
/// are skipped on insert, so their full-surface rect can't trip the
/// full-repaint coverage threshold on add or remove.
///
/// **Storage shape.** Per-paint snapshots don't live inline here — they
/// live in [`DamageEngine::paints`](crate::scene::damage::DamageEngine),
/// one [`BlockArena`](crate::common::block_arena::BlockArena) shared by
/// every widget, and this struct just holds a `Span` into it. Each row
/// is chrome (row 0 when present), one direct shape, or a child marker,
/// mirroring `LayerCascade::paint_arena`.
///
/// A widget takes a block when it enters the map and hands it back when
/// it leaves; a paint-count change is those two in sequence, and a
/// same-count refresh writes in place. **Nothing is ever relocated**, so
/// a span is stable for the snapshot's whole life and no pass has to be
/// able to reach the map to reclaim storage — see
/// [`crate::common::block_arena`] for why that matters more than the
/// slack the size classes cost.
///
/// **No cached `rect`.** The node's own paint extent — the union of its
/// `paint_arena` rows, folded by
/// [`PaintRows::union_screens`](crate::scene::cascade::paint::PaintRows::union_screens)
/// — is a pure function of `(hash, cascade_input)`:
/// every geometry input (`layout_rect`, ancestor transform/clip) lives
/// in `cascade_input` and every shape input lives in `hash`, so a
/// snapshot field would be a redundant cache of those two. The diff
/// keys the "node unchanged" fast path on `(hash, cascade_input)`
/// directly; the per-shape screen rects needed when something *did*
/// change are recovered from `paint_span`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct NodeSnapshot {
    /// Slice into [`DamageEngine::paints`](crate::scene::damage::DamageEngine)
    /// describing this
    /// widget's per-paint snapshots in record order (chrome at row 0
    /// when present, then shapes + child markers). Never empty — the
    /// row invariant means rowless nodes don't get an entry in `prev`
    /// at all.
    pub(super) paint_span: Span,
    /// Authoring hash from last frame's `Tree.rollups.node`.
    pub(crate) hash: ContentHash,
    /// Rollup hash of this node + its entire subtree from last frame's
    /// `Tree.rollups.subtree`. Pair with `cascade_input` to drive the
    /// subtree-skip fast path: if both match the current frame, every
    /// descendant is bit-identical and the per-node diff can jump to
    /// `subtree_end[i]`.
    ///
    /// Keyed by `WidgetId` rather than read from the cascade's
    /// node-indexed
    /// [`arena_hashes`](crate::scene::cascade::LayerCascade), which holds
    /// the same value: a widget outlives the index it occupied, and the
    /// frames where its index moves are the frames a full rebuild
    /// overwrites that column before this walk runs.
    pub(super) subtree_hash: ContentHash,
    /// Fingerprint of last frame's cascade inputs at this node (parent
    /// transform/clip/disabled/invisible + own arranged rect). See
    /// [`CascadeInputHash`].
    ///
    /// The cascade's `cascade_inputs[i]` is this frame's value, so the
    /// pair is one fact and its history rather than two owners: this
    /// walk compares them and then overwrites this one.
    pub(super) cascade_input: CascadeInputHash,
    /// Paint-order position: the immediate parent's `WidgetId` bits,
    /// or the layer discriminant for a root. A widget reparented (or
    /// moved to another layer) at an identical rect with identical
    /// content keeps `hash`, `subtree_hash`, AND `cascade_input`
    /// (which folds ancestor *state*, not identity) — yet its
    /// compositing order against outside overlappers flipped, so the
    /// skip tiers must not treat it as unchanged.
    pub(super) parent_key: u64,
}
