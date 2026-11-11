//! The per-frame memo from a recorded gradient to its atlas row.

use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
use crate::renderer::cpu_gradient_atlas::CpuGradientAtlas;
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::resolved_gradient::ResolvedGradient;
use crate::shape::paint::lowered_brush::LoweredBrush;
use crate::shape::record_store::recorded_gradient::RecordedGradient;
use crate::shape::record_store::recorded_gradients::GradientId;

/// Resolves each recorded gradient to its atlas row at most once per frame.
#[derive(Debug, Default)]
pub(super) struct GradientResolver {
    resolved: Vec<Option<ResolvedGradient>>,
}

impl GradientResolver {
    /// Opens the frame's pass over `gradients`, registering them in `atlas`, forgetting last
    /// frame's rows (another window may have evicted them).
    pub(super) fn begin<'a>(
        &'a mut self,
        gradients: &'a [RecordedGradient],
        atlas: &'a mut CpuGradientAtlas,
    ) -> GradientPass<'a> {
        self.resolved.clear();
        self.resolved.resize(gradients.len(), None);
        GradientPass {
            gradients,
            atlas,
            resolved: &mut self.resolved,
        }
    }
}

/// One frame's pass over the recorded gradients, memoizing each resolved row.
#[derive(Debug)]
pub(super) struct GradientPass<'a> {
    gradients: &'a [RecordedGradient],
    atlas: &'a mut CpuGradientAtlas,
    resolved: &'a mut Vec<Option<ResolvedGradient>>,
}

impl GradientPass<'_> {
    pub(super) fn source(&mut self, brush: LoweredBrush) -> BrushSource {
        match brush {
            LoweredBrush::Solid(color) => BrushSource::Solid(color),
            LoweredBrush::Gradient { id, .. } => BrushSource::Gradient(self.resolve(id)),
        }
    }

    /// The ramp recorded under `id`.
    pub(super) const fn ramp(&self, id: GradientId) -> &ColorRamp {
        &self.gradients[id.0 as usize].ramp
    }

    pub(super) fn resolve(&mut self, id: GradientId) -> ResolvedGradient {
        let idx = id.0 as usize;
        if let Some(resolved) = self.resolved[idx] {
            return resolved;
        }
        let gradient = &self.gradients[idx];
        let resolved = ResolvedGradient {
            axis: gradient.axis,
            lut_row: self.atlas.register(&gradient.ramp),
            kind: gradient.kind,
        };
        self.resolved[idx] = Some(resolved);
        resolved
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

        /// The atlas's `register` calls so far, which a memoized repeat leaves alone.
        pub(crate) fn registrations(&self) -> u32 {
            self.atlas.registrations()
        }
    }
}
