//! One declared input scope.

use crate::input::key_class::KeyFilter;
use crate::primitives::identity::widget_id::WidgetId;
use crate::scene::plane::Plane;

/// One declared input scope, in paint order across every plane; the table
/// [`crate::input_state::InputState`] resolves a key press against.
///
/// Not a column on [`EntryRow`](crate::cascade::rows::entry_row::EntryRow): scopes are a handful per frame, and
/// containment is answered by [`Cascade::is_within`](crate::cascade::Cascade::is_within).
/// Same lifecycle as [`HitRow`](crate::cascade::rows::hit_row::HitRow): cleared on a full rebuild, repopulated by the
/// non-incremental walk, retained across incremental runs with matching
/// subtree hashes.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScopeRow {
    pub(crate) plane: Plane,
    pub(crate) id: WidgetId,
    pub(crate) filter: KeyFilter,
}
