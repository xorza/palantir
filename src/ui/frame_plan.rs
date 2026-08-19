//! What `Ui::frame` decided to do this frame, and the facts it decided from.

use crate::display::Display;
use crate::input::policy::{InputPolicy, InputSignal};

/// What `Ui::frame` should do this frame, decided at entry
/// from fired wake reasons + input state + prior-frame validity.
/// `PaintOnly` and `FullRecord` are mutually exclusive by construction
/// — `paint_only ⇒ !force_full` is encoded in the variant shape
/// instead of relying on two independent bools.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FramePlan {
    /// Skip pre_record / record / finalize / layout / cascade and
    /// reuse the retained tree + cascade from the prior frame. Only
    /// fired by the anim-only fast path.
    PaintOnly,
    /// Run record + (optional) double-layout + finalize. `force_full`
    /// is true when the prior frame's damage snapshot must be
    /// discarded (surface change, missed submit, first frame).
    FullRecord { force_full: bool },
}

#[derive(Clone, Copy, Debug)]
pub(super) struct FrameClassifyInput {
    pub(super) display: Display,
    pub(super) damage_baseline_valid: bool,
    pub(super) input_policy: InputPolicy,
    pub(super) input_signal: InputSignal,
    pub(super) close_requested: bool,
}
