//! What the damage diff remembers about one widget between frames.

use crate::cascade::cascade_input_hash::CascadeInputHash;
use crate::common::content_hash::ContentHash;
use crate::common::span::Span;

/// Per-widget snapshot in [`crate::damage::engine::DamageEngine::prev`], keyed
/// by [`WidgetId`](crate::primitives::identity::widget_id::WidgetId). Only
/// widgets with paint rows get an entry, so a rowless node cannot trip the
/// full-repaint threshold on add or remove. Per-paint snapshots live in
/// [`DamageEngine::paints`](crate::damage::engine::DamageEngine) and this holds
/// a stable `Span` into it. No cached paint extent: it is a pure function of
/// `(hash, cascade_input)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct NodeSnapshot {
    /// This widget's per-paint snapshots in record order (chrome first). Never empty.
    pub(super) paint_span: Span,
    /// Last frame's node authoring hash.
    pub(crate) hash: ContentHash,
    /// Last frame's rollup hash of this node and its subtree. With
    /// `cascade_input`, a match means the diff can jump to `subtree_end[i]`.
    /// Keyed by `WidgetId` because a widget outlives its index.
    pub(super) subtree_hash: ContentHash,
    /// Fingerprint of last frame's cascade inputs at this node; see
    /// [`CascadeInputHash`]. This walk compares it and then overwrites it.
    pub(super) cascade_input: CascadeInputHash,
    /// Paint-order position: the parent's `WidgetId` bits, or the layer
    /// discriminant for a root. A reparented widget keeps every hash but its
    /// compositing order flipped, so the skip tiers must not skip it.
    pub(super) parent_key: u64,
}
