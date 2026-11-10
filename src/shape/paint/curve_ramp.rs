//! What a lowered curve's color multiplies by.

use crate::common::content_hash::ContentHash;
use crate::shape::record_store::recorded_gradients::GradientId;

/// What a curve's stroke color multiplies: nothing, or an interned ramp.
///
/// Inline pair, not a struct, so the value is 16 B in every `LoweredStroke`.
#[derive(Clone, Copy, Debug)]
pub(crate) enum CurveRamp {
    None,
    Interned { id: GradientId, hash: ContentHash },
}
