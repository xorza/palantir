//! The per-frame memo from a recorded gradient to its atlas row.

use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::resolved_gradient::ResolvedGradient;
use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
use crate::shape::paint::shape_brush::ShapeBrush;
use crate::shape::record_store::recorded_gradient::RecordedGradient;
use crate::shape::record_store::recorded_gradients::GradientId;

/// Resolves each recorded gradient to its atlas row at most once per frame.
#[derive(Debug)]
pub(super) struct GradientResolver {
    atlas: SharedGradientAtlas,
    resolved: Vec<Option<ResolvedGradient>>,
}

impl GradientResolver {
    pub(super) const fn new(atlas: SharedGradientAtlas) -> Self {
        Self {
            atlas,
            resolved: Vec::new(),
        }
    }

    /// Opens the frame's pass over `gradients`, forgetting last frame's rows (another window may have evicted them).
    pub(super) fn begin<'a>(&'a mut self, gradients: &'a [RecordedGradient]) -> GradientPass<'a> {
        self.resolved.clear();
        self.resolved.resize(gradients.len(), None);
        GradientPass {
            gradients,
            atlas: &self.atlas,
            resolved: &mut self.resolved,
        }
    }
}

/// One frame's pass over the recorded gradients, memoizing each resolved row.
#[derive(Debug)]
pub(super) struct GradientPass<'a> {
    gradients: &'a [RecordedGradient],
    atlas: &'a SharedGradientAtlas,
    resolved: &'a mut Vec<Option<ResolvedGradient>>,
}

impl GradientPass<'_> {
    pub(super) fn source(&mut self, brush: ShapeBrush) -> BrushSource {
        match brush {
            ShapeBrush::Solid(color) => BrushSource::Solid(color),
            ShapeBrush::Gradient { id, .. } => BrushSource::Gradient(self.resolve(id)),
        }
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
    use crate::renderer::frontend::encoder::gradient_resolver::GradientPass;
    use crate::shape::record_store::recorded_gradients::GradientId;

    impl GradientPass<'_> {
        /// Whether this pass has resolved `id` yet.
        pub(crate) fn is_resolved(&self, id: GradientId) -> bool {
            self.resolved[id.0 as usize].is_some()
        }
    }
}
