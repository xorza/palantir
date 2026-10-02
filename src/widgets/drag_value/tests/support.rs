//! The deferred-commit frame a drag test drives, and the signals it counts.

use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::ui::harness::UiHarness;
use crate::ui::harness::passes::Passes;
use crate::widgets::configure::Configure;
use crate::widgets::drag_value::DragValue;
use crate::widgets::value_response::test_support::ValueEdges;

/// Drive one frame of a `DragValue` through a commit-deferring caller:
/// the draft re-seeds from `canonical` every record pass and is adopted
/// only on `committed` — the undo-aware consumption pattern the commit
/// signal exists for. One snapshot per record pass: the edges show in
/// pass A, and a commit must fire in exactly one pass or a per-pass
/// consumer (an undo pusher) double-applies.
pub(super) fn deferred_frame(
    h: &mut UiHarness,
    id: WidgetId,
    canonical: &mut f64,
    editable: bool,
    disabled: bool,
) -> Passes<ValueEdges> {
    h.frame_passes(|ui| {
        let mut draft = *canonical;
        let r = DragValue::new(&mut draft)
            .editable(editable)
            .disabled(disabled)
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
