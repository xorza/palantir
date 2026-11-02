//! The deferred-commit frame a drag test drives, and the signals it counts.

use crate::host::harness::Harness;
use crate::host::harness::internals::Passes;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::widget_core::configure::Configure;
use crate::widget_core::value_response::ValueEdges;
use crate::widgets::drag_value::DragValue;

/// The settings a test frame adds to the drag value it shows.
pub(super) type Settings = for<'a> fn(DragValue<'a>) -> DragValue<'a>;

/// A drag value that a click opens to typing.
pub(super) const TYPED: Settings = |d| d.editable(true);

/// A drag value that only scrubs.
pub(super) const SCRUB: Settings = |d| d.editable(false);

/// Drive one frame of a `DragValue` through a commit-deferring caller, with `set` applied: the draft re-seeds from `canonical` every record pass and is adopted only on `committed` (the undo-aware pattern). One snapshot per record pass: edges show in pass A, and a commit must fire in exactly one pass or a per-pass consumer double-applies.
pub(super) fn deferred_frame(
    h: &mut Harness,
    id: WidgetId,
    canonical: &mut f64,
    set: Settings,
) -> Passes<ValueEdges> {
    h.frame_passes(|ui| {
        let mut draft = *canonical;
        let r = set(DragValue::new(&mut draft))
            .speed(1.0)
            .decimals(2)
            .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
            .id(id)
            .show(ui);
        if r.committed {
            *canonical = draft;
        }
        r.edges()
    })
}
