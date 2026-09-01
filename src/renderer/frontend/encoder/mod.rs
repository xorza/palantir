//! The encode pass: walk the cascaded scene and turn each node's shapes
//! into paint calls, one layer at a time ([`layer_ctx`]).
//!
//! [`Encoder`] holds what the walk retains between frames, [`geometry`]
//! owns the rect math every shape kind resolves through, and
//! [`GradientResolver`] resolves each gradient once per frame rather than
//! once per shape that names it.

#[cfg(debug_assertions)]
mod collision_overlay;
mod geometry;
mod layer_ctx;

use crate::renderer::frontend::FrameScene;
use crate::renderer::frontend::encoder::layer_ctx::LayerCtx;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::resolved_gradient::ResolvedGradient;
use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
use crate::renderer::render_plan::RenderPlan;
use crate::scene::damage::Damage;
use crate::scene::record_store::recorded_gradient::RecordedGradient;
use crate::scene::shapes::paint::ShapeBrush;

/// Retained encoder state.
#[derive(Debug)]
pub(crate) struct Encoder {
    gradients: GradientResolver,
    gradient_atlas: SharedGradientAtlas,
}

/// Entries reset each encode because another window may have evicted their atlas rows.
#[derive(Debug, Default)]
struct GradientResolver {
    resolved: Vec<Option<ResolvedGradient>>,
}

impl GradientResolver {
    fn reset_for(&mut self, gradient_count: usize) {
        self.resolved.clear();
        self.resolved.resize(gradient_count, None);
    }

    fn source(
        &mut self,
        gradients: &[RecordedGradient],
        atlas: &SharedGradientAtlas,
        brush: ShapeBrush,
    ) -> BrushSource {
        let id = match brush {
            ShapeBrush::Solid(color) => return BrushSource::Solid(color),
            ShapeBrush::Gradient(id) => id,
        };
        let idx = id.0 as usize;
        if let Some(resolved) = self.resolved[idx] {
            return BrushSource::Gradient(resolved);
        }
        let gradient = &gradients[idx];
        let resolved = ResolvedGradient {
            axis: gradient.axis,
            lut_row: atlas.register_stops(&gradient.stops, gradient.interp),
            kind: gradient.kind,
        };
        self.resolved[idx] = Some(resolved);
        BrushSource::Gradient(resolved)
    }
}

impl Encoder {
    pub(crate) fn new(gradient_atlas: SharedGradientAtlas) -> Self {
        Self {
            gradients: GradientResolver::default(),
            gradient_atlas,
        }
    }

    /// Walk every tree in the scene forest in paint order, emitting logical-px
    /// paint commands into `out`. No GPU work, no scale/snap math — that
    /// lives in the composer + backend. Per-tree layout rows come off
    /// the scene layout, cascade rows off the scene cascade, keyed by layer.
    ///
    /// `plan` is the paint plan for this frame:
    /// - `Damage::Full` paints everything (first frame, surface change,
    ///   full-repaint fallback).
    /// - `Damage::Partial(damage)` runs damage-aware subtree
    ///   culling: a node whose `paint_rect` doesn't intersect any rect in
    ///   `region` short-circuits the whole subtree's recursion *and* its
    ///   Push/Pop emission. Caller's responsibility to skip the call
    ///   entirely when there's no damage to paint.
    ///
    /// The sink arrives ready for a fresh frame — a `ComposeSession` from
    /// `Composer::begin`, or an empty capturing sink.
    ///
    /// Deliberately carries no profiling span: the sink composes
    /// inline, so this covers the same work as [`Frontend::build`] and a
    /// second span would only read as an encoder regression against a
    /// pre-fusion capture.
    ///
    /// [`Frontend::build`]: crate::renderer::frontend::Frontend::build
    pub(crate) fn encode(
        &mut self,
        scene: &FrameScene<'_>,
        plan: RenderPlan,
        out: &mut impl PaintSink,
    ) {
        let Self {
            gradients: gradient_resolver,
            gradient_atlas,
        } = self;

        let damage_filter = match &plan.damage {
            Damage::Partial(damage) => Some(&damage.region),
            Damage::Full => None,
        };

        let viewport = scene.display.logical_rect();
        let now = scene.time;
        let gradients = scene.forest.record_store.gradients.records.as_slice();
        gradient_resolver.reset_for(gradients.len());
        // Matches the backend's padded physical scissor; both derive from
        // `renderer::render_plan::RenderPlan::AA_PADDING`.
        let damage_cull_margin = RenderPlan::cull_margin(scene.display.scale_factor);
        for (layer, tree) in scene.forest.trees.iter_paint_order() {
            let layer_cascades = &scene.cascade.layers[layer];
            let mut ctx = LayerCtx {
                tree,
                layout: &scene.layout[layer],
                cascade_inputs: layer_cascades.cascade_inputs.as_slice(),
                subtree_paint_rects: layer_cascades.subtree_paint_rects.as_slice(),
                gradients,
                gradient_atlas,
                gradient_resolver,
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
        collision_overlay::emit(scene.forest, scene.layout, out);
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use crate::renderer::frontend::FrameScene;
    use crate::renderer::frontend::capture::PaintCapture;
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
