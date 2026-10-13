//! One node's row in the cascade's response-lookup table.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::translate_scale::TranslateScale;

/// One per-node cascade row, in `Vec<EntryRow>` on
/// [`Cascade::entries`](crate::cascade::Cascade::entries).
///
/// Array-of-structs on purpose: every consumer is a single-index gather
/// ([`crate::input::input_state::InputState::response_for`] reads all three
/// fields once per widget per frame), so interleaving them puts a lookup on
/// one cache line, where `soa_rs` columns used three. The opposite call from
/// `Tree.records: Soa<NodeRecord>`, whose columns the measure/arrange passes
/// walk. Hit testing reads [`HitRow`](crate::cascade::rows::hit_row::HitRow), not this table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EntryRow {
    /// Visible screen rect (post-transform, clipped by ancestor clip).
    pub(crate) rect: Rect,
    /// The cumulative ancestor transform mapping this node's layout rect into
    /// unclipped surface space, `IDENTITY` when untransformed. Surfaced via
    /// `ResponseState::transform` to localize surface-space vectors.
    pub(crate) transform: TranslateScale,
    /// Effective disabled (self or any ancestor). Stored, not acted on:
    /// `ResponseState::merge_disabled` empties a disabled widget's response.
    pub(crate) disabled: bool,
}
