//! The encode pass: walk the cascaded scene into paint calls, one layer at a time ([`layer_ctx`]);
//! [`geometry`] owns the shared rect math, [`GradientResolver`] resolves each gradient once per frame.

#[cfg(debug_assertions)]
mod collision_overlay;
mod geometry;
mod layer_ctx;

use crate::damage::Damage;
use crate::renderer::frontend::FrameScene;
use crate::renderer::frontend::encoder::layer_ctx::LayerCtx;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::resolved_gradient::ResolvedGradient;
use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
use crate::renderer::render_plan::RenderPlan;
use crate::scene::record_store::recorded_gradient::RecordedGradient;
use crate::scene::record_store::recorded_gradients::GradientId;
use crate::shape::paint::shape_brush::ShapeBrush;

#[derive(Debug)]
pub(crate) struct Encoder {
    gradients: GradientResolver,
}

#[derive(Debug)]
struct GradientResolver {
    atlas: SharedGradientAtlas,
    resolved: Vec<Option<ResolvedGradient>>,
}

impl GradientResolver {
    /// Opens the frame's pass over `gradients`, forgetting last frame's rows (another window may have evicted them).
    fn begin<'a>(&'a mut self, gradients: &'a [RecordedGradient]) -> GradientPass<'a> {
        self.resolved.clear();
        self.resolved.resize(gradients.len(), None);
        GradientPass {
            gradients,
            atlas: &self.atlas,
            resolved: &mut self.resolved,
        }
    }
}

#[derive(Debug)]
pub(super) struct GradientPass<'a> {
    gradients: &'a [RecordedGradient],
    atlas: &'a SharedGradientAtlas,
    resolved: &'a mut Vec<Option<ResolvedGradient>>,
}

impl GradientPass<'_> {
    fn source(&mut self, brush: ShapeBrush) -> BrushSource {
        match brush {
            ShapeBrush::Solid(color) => BrushSource::Solid(color),
            ShapeBrush::Gradient { id, .. } => BrushSource::Gradient(self.resolve(id)),
        }
    }

    fn resolve(&mut self, id: GradientId) -> ResolvedGradient {
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

impl Encoder {
    pub(crate) const fn new(gradient_atlas: SharedGradientAtlas) -> Self {
        Self {
            gradients: GradientResolver {
                atlas: gradient_atlas,
                resolved: Vec::new(),
            },
        }
    }

    /// Walks every tree in paint order, emitting logical-px paint commands into `out`. `Damage::Partial` culls
    /// subtrees whose `paint_rect` intersects no damage rect; callers skip the call when nothing is damaged.
    ///
    /// No profiling span: [`Frontend::build`]'s covers it, as the sink composes inline.
    ///
    /// [`Frontend::build`]: crate::renderer::frontend::Frontend::build
    pub(crate) fn encode(
        &mut self,
        scene: &FrameScene<'_>,
        plan: RenderPlan,
        out: &mut impl PaintSink,
    ) {
        let damage_filter = match &plan.damage {
            Damage::Partial(damage) => Some(&damage.region),
            Damage::Full => None,
        };

        let viewport = scene.display.logical_rect();
        let now = scene.time;
        let mut gradients = self
            .gradients
            .begin(scene.forest.record_store.gradients.records.as_slice());
        // Matches the backend's padded physical scissor; both derive from `RenderPlan::AA_PADDING`.
        let damage_cull_margin = RenderPlan::cull_margin(scene.display.scale_factor());
        for (layer, tree) in scene.forest.trees.iter_paint_order() {
            let layer_cascades = &scene.cascade.layers[layer];
            let mut ctx = LayerCtx {
                tree,
                layout: &scene.layout[layer],
                cascade_inputs: layer_cascades.cascade_inputs.as_slice(),
                subtree_paint_rects: layer_cascades.subtree_paint_rects.as_slice(),
                gradients: &mut gradients,
                paint_anim_cursor: tree.paint_anims.cursor(),
                gpu_views: scene.gpu_views,
                damage_filter,
                damage_cull_margin,
                viewport,
                now,
            };
            for root in &tree.roots {
                ctx.encode_node(root.first_node, out);
            }
        }

        #[cfg(debug_assertions)]
        collision_overlay::emit(scene.forest, scene.layout, scene.cascade, out);
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::internals::paint_capture::PaintCapture;
    use crate::renderer::frontend::FrameScene;
    use crate::renderer::frontend::encoder::Encoder;
    use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
    use crate::renderer::render_plan::RenderPlan;

    pub(crate) fn encode(
        scene: FrameScene<'_>,
        gradient_atlas: &SharedGradientAtlas,
        plan: RenderPlan,
    ) -> PaintCapture {
        let mut encoder = Encoder::new(gradient_atlas.clone());
        let mut recorded = PaintCapture::default();
        encoder.encode(&scene, plan, &mut recorded);
        recorded
    }
}

#[cfg(test)]
mod tests;
