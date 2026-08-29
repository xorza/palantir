//! The memoized trailing advance of the truncation ellipsis.

use crate::text::key::QuantizedFace;

/// Memoized trailing advance of "…" for one face.
///
/// `Default` only to satisfy `tinyvec`'s `Array` bound — it fills the
/// unused tail of [`CosmicMeasure::ellipsis_advance`](crate::text::cosmic::CosmicMeasure::ellipsis_advance), which `len` keeps out of
/// every read. A zeroed memo could not match a live face anyway:
/// `quantize_metric` floors `size_q` at 1.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct EllipsisMemo {
    face: QuantizedFace,
    advance: f32,
}

impl EllipsisMemo {
    /// A face to look up, with no advance measured for it yet.
    pub(super) fn wanted(face: QuantizedFace) -> Self {
        Self { face, advance: 0.0 }
    }

    /// This memo's advance, if it was shaped at `face`. `None` is the
    /// miss that makes the caller shape one.
    pub(super) fn advance_for(&self, face: QuantizedFace) -> Option<f32> {
        (self.face == face).then_some(self.advance)
    }

    /// `self` with the advance that was just measured for it.
    pub(super) fn measured(self, advance: f32) -> Self {
        Self { advance, ..self }
    }
}
