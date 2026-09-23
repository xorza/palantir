//! One interned gradient's retained content.

use crate::primitives::brush::gradient::FillAxis;
use crate::primitives::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::fill_kind::FillKind;

/// Retained gradient content. The physical atlas row is resolved while
/// encoding because the shared atlas may evict rows between window frames.
#[derive(Clone, Debug)]
pub(crate) struct RecordedGradient {
    pub(crate) axis: FillAxis,
    pub(crate) kind: FillKind,
    pub(crate) ramp: ColorRamp,
}

impl PartialEq for RecordedGradient {
    fn eq(&self, other: &Self) -> bool {
        // Raw equality is the hot path; unpacking also collapses canonical ±0.
        (self.axis == other.axis || self.axis.lanes() == other.axis.lanes())
            && self.kind == other.kind
            && self.ramp == other.ramp
    }
}
