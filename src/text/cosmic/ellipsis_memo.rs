//! The memoized trailing advance of the truncation ellipsis.

use crate::text::key::QuantizedFace;

/// Memoized trailing advance of "…" for one face.
///
/// `Default` only for `tinyvec`'s `Array` bound, filling the unused tail of [`CosmicMeasure::ellipsis_advance`](crate::text::cosmic::CosmicMeasure::ellipsis_advance); a zeroed memo can't match a live face since `quantize_metric` floors `size_q` at 1.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct EllipsisMemo {
    pub(super) face: QuantizedFace,
    pub(super) advance: f32,
}

impl EllipsisMemo {
    /// This memo's advance if shaped at `face`; `None` makes the caller shape one.
    pub(super) fn advance_for(&self, face: QuantizedFace) -> Option<f32> {
        (self.face == face).then_some(self.advance)
    }
}
