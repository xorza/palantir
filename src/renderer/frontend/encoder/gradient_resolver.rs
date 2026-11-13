//! The per-frame memo from a recorded gradient to its row.

use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::paint::lut_row::LutRow;
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::resolved_gradient::ResolvedGradient;
use crate::renderer::row_table::RowTable;
use crate::shape::paint::lowered_brush::LoweredBrush;
use crate::shape::record_store::recorded_gradient::RecordedGradient;
use crate::shape::record_store::recorded_gradients::GradientId;

/// Resolves each recorded gradient to its row at most once per frame.
#[derive(Debug, Default)]
pub(super) struct GradientResolver {
    /// Each recorded gradient's row, once this frame resolved it; its axis and kind stay in the
    /// record.
    resolved: Vec<Option<LutRow>>,
}

impl GradientResolver {
    /// Opens the frame's pass over `gradients`, registering them in `rows`, forgetting last
    /// frame's rows (another window may have evicted them).
    pub(super) fn begin<'a>(
        &'a mut self,
        gradients: &'a [RecordedGradient],
        rows: &'a mut RowTable,
    ) -> GradientPass<'a> {
        self.resolved.clear();
        self.resolved.resize(gradients.len(), None);
        GradientPass {
            gradients,
            rows,
            resolved: &mut self.resolved,
        }
    }
}

/// One frame's pass over the recorded gradients, memoizing each resolved row.
#[derive(Debug)]
pub(super) struct GradientPass<'a> {
    gradients: &'a [RecordedGradient],
    rows: &'a mut RowTable,
    resolved: &'a mut Vec<Option<LutRow>>,
}

impl GradientPass<'_> {
    pub(super) fn source(&mut self, brush: LoweredBrush) -> BrushSource {
        match brush {
            LoweredBrush::Solid(color) => BrushSource::Solid(color),
            LoweredBrush::Gradient { id, .. } => BrushSource::Gradient(self.resolve(id)),
        }
    }

    pub(super) const fn ramp(&self, id: GradientId) -> &ColorRamp {
        &self.gradients[id.0 as usize].ramp
    }

    pub(super) fn resolve(&mut self, id: GradientId) -> ResolvedGradient {
        let idx = id.0 as usize;
        let gradient = &self.gradients[idx];
        let lut_row = *self.resolved[idx].get_or_insert_with(|| self.rows.register(&gradient.ramp));
        ResolvedGradient {
            axis: gradient.axis,
            lut_row,
            kind: gradient.kind,
        }
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::internals::scratch_report;
    use crate::renderer::frontend::encoder::gradient_resolver::{GradientPass, GradientResolver};
    use crate::shape::record_store::recorded_gradients::GradientId;

    impl GradientResolver {
        pub(crate) fn scratch_bytes(&self) -> usize {
            let Self { resolved } = self;
            scratch_report::vec_bytes(resolved)
        }
    }

    impl GradientPass<'_> {
        /// Whether this pass has resolved `id` yet.
        pub(crate) fn is_resolved(&self, id: GradientId) -> bool {
            self.resolved[id.0 as usize].is_some()
        }

        /// The table's `register` calls so far, which a memoized repeat leaves alone.
        pub(crate) fn registrations(&self) -> u32 {
            self.rows.registrations()
        }
    }
}
