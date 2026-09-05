//! The deferred-commit frame a drag test drives, and the signals it counts.

use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::ui::harness::UiHarness;
use crate::widgets::configure::Configure;
use crate::widgets::drag_value::DragValue;

#[derive(Debug)]
pub(super) struct Signals {
    pub(super) changed: bool,
    pub(super) committed: bool,
    /// How many record passes reported `committed` this frame. A frame
    /// records twice on action input; a commit must fire in exactly one
    /// pass or a per-pass consumer (an undo pusher) double-applies.
    pub(super) commits: u32,
}

/// Drive one frame of a `DragValue` through a commit-deferring caller:
/// the draft re-seeds from `canonical` every record pass and is adopted
/// only on `committed` — the undo-aware consumption pattern the commit
/// signal exists for. `changed`/`committed` OR-accumulate across the
/// frame's record passes (one-frame edges only show in the first pass);
/// `commits` counts per pass so a double-fire is visible.
pub(super) fn deferred_frame(
    h: &mut UiHarness,
    id: WidgetId,
    canonical: &mut f64,
    editable: bool,
    disabled: bool,
) -> Signals {
    let mut s = Signals {
        changed: false,
        committed: false,
        commits: 0,
    };
    h.frame(|ui| {
        let mut draft = *canonical;
        let r = DragValue::new(&mut draft)
            .editable(editable)
            .disabled(disabled)
            .speed(1.0)
            .decimals(2)
            .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
            .id(id)
            .show(ui);
        s.changed |= r.changed;
        if r.committed {
            s.committed = true;
            s.commits += 1;
            *canonical = draft;
        }
    });
    s
}
