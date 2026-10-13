//! One interactive node's row in the hit-test table.

use crate::input::sense::Sense;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;

/// One interactive row: a node whose effective `sense` is nonempty or which is
/// focusable. Pushed in paint order, so a reverse scan yields topmost-first.
///
/// Self-sufficient on purpose: carrying geometry and gates makes a hit test a
/// dense sequential scan over interactive rows, instead of a random access into
/// the all-node [`Cascade::entries`](crate::cascade::Cascade::entries) per candidate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct HitRow {
    /// Visible screen rect, the same as `EntryRow::rect`; duplicated to keep
    /// the scan sequential.
    pub(crate) rect: Rect,
    pub(crate) widget_id: WidgetId,
    /// Pointer interactions this row participates in (`HOVER` / `CLICK` /
    /// `DRAG` / `SCROLL`), already cascaded: `Sense::NONE` for an invisible
    /// subtree, as declared for a disabled one, which keeps absorbing what it
    /// covers.
    pub(crate) sense: Sense,
    /// Focus eligibility, checked by the focusable hit-test only; always
    /// `false` in a disabled or invisible subtree.
    pub(crate) focusable: bool,
    /// Disabled (this widget or any ancestor), as [`EntryRow::disabled`](crate::cascade::rows::entry_row::EntryRow::disabled). The
    /// pointer walks ignore it, since `ResponseState::merge_disabled` empties
    /// what a disabled row reads. Focus reads it:
    /// [`Cascade::hit_test_press`](crate::cascade::Cascade::hit_test_press) ends its walk
    /// here, so a press this row absorbed focuses nothing beneath it.
    pub(crate) disabled: bool,
}
