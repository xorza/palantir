//! Pairing a node's text records with the shapes the measure pass
//! produced for them.

use crate::layout::{LayerLayout, ShapedText};
use crate::primitives::span::Span;
use crate::scene::shapes::record::ShapeRecord;

/// One node's shaped-text runs, handed out in record order.
///
/// The measure pass stamps [`LayerLayout::text_shapes`] in the same order a
/// walk meets `ShapeRecord::Text` going through the node's shapes, so
/// pairing the two is a cursor rather than a lookup.
///
/// **It advances on every text record, including ones the walker then
/// drops.** A dropped run still owns its slot, and skipping it would slide
/// every later run on the node onto the wrong shape. So the discrimination
/// lives here, ahead of whatever the caller gates on: a walk hands each
/// record over and gets an answer, rather than deciding for itself which
/// records count.
///
/// Both walks over a node's shapes use this — the encoder's paint emission
/// and cascade's paint-rect rollup. They read different fields off the
/// answer (`key` and `measured` against `measured` alone) but they consume
/// the same column in the same order, and the bounds and drained checks
/// below are the contract that keeps them agreeing.
#[derive(Debug)]
pub(crate) struct TextRuns {
    /// The node's slice of [`LayerLayout::text_shapes`].
    span: Span,
    /// How many of it have been handed out.
    taken: u32,
}

impl TextRuns {
    pub(crate) fn new(span: Span) -> Self {
        Self { span, taken: 0 }
    }

    /// What `record` measured to, or `None` when it is not a text record.
    pub(crate) fn shaped(
        &mut self,
        record: &ShapeRecord,
        layout: &LayerLayout,
    ) -> Option<ShapedText> {
        if !matches!(record, ShapeRecord::Text { .. }) {
            return None;
        }
        debug_assert!(
            self.taken < self.span.len,
            "a text shape has no matching ShapedText entry: ordinal {} against span len {}",
            self.taken,
            self.span.len,
        );
        let shaped = layout.text_shapes[(self.span.start + self.taken) as usize];
        self.taken += 1;
        Some(shaped)
    }

    /// Every run the node's span holds was handed out — what the measure
    /// pass stamped and what the walk met are the same count.
    pub(crate) fn is_drained(&self) -> bool {
        self.taken == self.span.len
    }
}
