//! Pairing a node's text records with the shapes the measure pass produced.

use crate::common::span::Span;
use crate::layout::layer_layout::LayerLayout;
use crate::layout::text::shaped_text::ShapedText;
use crate::shape::record::ShapeRecord;

/// One node's shaped-text runs, handed out in record order.
///
/// Measure stamps [`LayerLayout::text_shapes`] in the order a walk meets `ShapeRecord::Text`, so pairing is a cursor.
///
/// **It advances on every text record, including ones the walker drops**: a dropped run still owns its slot, and skipping it would shift later runs onto the wrong shape. Callers hand over each record rather than deciding which count.
///
/// The encoder's paint emission and cascade's paint-rect rollup both use it; the bounds and drained checks below keep them in agreement.
#[derive(Debug)]
pub(crate) struct TextRuns {
    /// The node's slice of [`LayerLayout::text_shapes`].
    span: Span,
    /// How many of it have been handed out.
    taken: u32,
}

impl TextRuns {
    pub(crate) const fn new(span: Span) -> Self {
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

    /// Every run the node's span holds was handed out: measure's stamped count equals the walk's.
    pub(crate) const fn is_drained(&self) -> bool {
        self.taken == self.span.len
    }
}
