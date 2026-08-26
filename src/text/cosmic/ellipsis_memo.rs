//! The memoized trailing advance of the truncation ellipsis.

use crate::text::{FontFamily, FontWeight};

/// Memoized trailing advance of "…" for one face.
///
/// `Default` only to satisfy `tinyvec`'s `Array` bound — it fills the
/// unused tail of [`CosmicMeasure::ellipsis_advance`](crate::text::cosmic::CosmicMeasure::ellipsis_advance), which `len` keeps out of
/// every read. A zeroed memo could not match a live face anyway:
/// `quantize_metric` floors `size_q` at 1.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct EllipsisMemo {
    size_q: u32,
    family_q: u8,
    weight_q: u8,
    advance: f32,
}

impl EllipsisMemo {
    /// A face to look up, with no advance measured for it yet.
    pub(super) fn wanted(size_q: u32, family: FontFamily, weight: FontWeight) -> Self {
        Self {
            size_q,
            family_q: family as u8,
            weight_q: weight as u8,
            advance: 0.0,
        }
    }

    /// This memo's advance, if it was shaped from the same face at the
    /// same size as `want`. `None` is the miss that makes the caller
    /// shape one.
    pub(super) fn advance_for(&self, want: &Self) -> Option<f32> {
        (self.size_q == want.size_q
            && self.family_q == want.family_q
            && self.weight_q == want.weight_q)
            .then_some(self.advance)
    }

    /// `want` with the advance that was just measured for it.
    pub(super) fn measured(self, advance: f32) -> Self {
        Self { advance, ..self }
    }
}
