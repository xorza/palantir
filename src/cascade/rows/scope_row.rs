//! One declared input scope.

use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::key_class::KeyFilter;
use crate::scene::plane::Plane;

/// One declared input scope, in paint order across every plane; the table
/// [`crate::input::Input`] resolves a key press against.
///
/// Not a column on [`EntryRow`](crate::cascade::rows::entry_row::EntryRow): scopes are a handful per frame, and
/// containment is answered by [`Cascade::is_within`](crate::cascade::Cascade::is_within).
/// Cleared on a full rebuild and refilled by that walk; every refresh keeps it as is, as the
/// cascade key proves it unchanged, where it rewrites the rects of
/// [`HitRow`](crate::cascade::rows::hit_row::HitRow)s.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScopeRow {
    pub(crate) plane: Plane,
    pub(crate) id: WidgetId,
    pub(crate) filter: KeyFilter,
    /// An overlay's claim on its layer ([`OverlayScope::claim`](crate::widget::OverlayScope::claim)):
    /// it cuts every plane below its own off. A widget's own scope, as a button's Space and
    /// Enter, says only which keys it takes.
    pub(crate) blocks_below: bool,
}
