//! One interned gradient's retained content.

use crate::primitives::brush::gradient::stops::GradientStops;
use crate::primitives::brush::gradient::{FillAxis, Interp};
use crate::primitives::fill_kind::FillKind;

/// Retained gradient content. The physical atlas row is resolved while
/// encoding because the shared atlas may evict rows between window frames.
#[derive(Clone, Debug)]
pub(crate) struct RecordedGradient {
    pub(crate) axis: FillAxis,
    pub(crate) kind: FillKind,
    pub(crate) stops: GradientStops,
    pub(crate) interp: Interp,
}

impl PartialEq for RecordedGradient {
    fn eq(&self, other: &Self) -> bool {
        // Raw equality is the hot path; unpacking also collapses canonical ±0.
        (self.axis == other.axis || self.axis.lanes() == other.axis.lanes())
            && self.kind == other.kind
            && self.stops == other.stops
            && self.interp == other.interp
    }
}
