//! The encode pass: walk the cascaded scene into paint calls, one plane at a time ([`plane_ctx`]);
//! [`gradient_resolver`] resolves each gradient once per frame.

#[cfg(debug_assertions)]
mod collision_overlay;
mod gradient_resolver;
mod plane_ctx;

use crate::damage::Damage;
use crate::renderer::frontend::FrameScene;
use crate::renderer::frontend::encoder::gradient_resolver::GradientResolver;
use crate::renderer::frontend::encoder::plane_ctx::PlaneCtx;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
use crate::renderer::render_plan::RenderPlan;

/// The encode pass's retained state: the per-frame gradient memo, kept for its capacity.
#[derive(Debug)]
pub(crate) struct Encoder {
    gradients: GradientResolver,
}

impl Encoder {
    pub(crate) const fn new(gradient_atlas: SharedGradientAtlas) -> Self {
        Self {
            gradients: GradientResolver::new(gradient_atlas),
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

        let viewport = scene.stamp.display.logical_rect();
        let now = scene.stamp.time;
        let mut gradients = self
            .gradients
            .begin(scene.forest.record_store.gradients.records.as_slice());
        // Matches the backend's padded physical scissor; both derive from `RenderPlan::AA_PADDING`.
        let damage_cull_margin = RenderPlan::cull_margin(scene.stamp.display.scale_factor());
        for (plane, tree) in scene.forest.trees.iter_paint_order() {
            let plane_cascades = &scene.cascade.planes[plane];
            let mut ctx = PlaneCtx {
                tree,
                layout: &scene.layout[plane],
                cascade_inputs: plane_cascades.cascade_inputs.as_slice(),
                subtree_paint_rects: plane_cascades.subtree_paint_rects.as_slice(),
                gradients: &mut gradients,
                paint_animation_cursor: tree.paint_animations.cursor(),
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
        collision_overlay::emit(scene, out);
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::internals::paint_capture::PaintCapture;
    use crate::renderer::frontend::FrameScene;
    use crate::renderer::frontend::encoder::Encoder;
    use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
    use crate::renderer::render_plan::RenderPlan;

    impl Encoder {
        pub(crate) fn scratch_bytes(&self) -> usize {
            let Self { gradients } = self;
            gradients.scratch_bytes()
        }
    }

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
