//! What a lowered curve's colour multiplies by.

use crate::scene::record_store::recorded_gradients::GradientId;

/// What a curve's stroke colour multiplies: nothing, or an interned ramp.
///
/// Inline pair, not a struct, so the value is 16 B and `ShapeRecord::Curve` fits its 88 B.
#[derive(Clone, Copy, Debug)]
pub(crate) enum CurveRamp {
    None,
    Interned { id: GradientId, hash: u64 },
}
