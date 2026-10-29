//! The encode pass: walk the cascaded scene into paint calls, one plane at a time ([`plane_ctx`]);
//! [`gradient_resolver`] resolves each gradient once per frame.

#[cfg(debug_assertions)]
mod collision_overlay;
mod cull_region;
mod gradient_resolver;
mod plane_ctx;

use crate::damage::Damage;
use crate::primitives::geometry::rect::Rect;
use crate::renderer::frontend::encoder::cull_region::CullRegion;
use crate::renderer::frontend::encoder::gradient_resolver::GradientResolver;
use crate::renderer::frontend::encoder::plane_ctx::PlaneCtx;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::{BuildPlan, FrameScene};
use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
use crate::renderer::render_plan::RenderPlan;

/// The encode pass's retained state: the per-frame gradient memo and the footprints a partial
/// frame encodes, kept for their capacity.
#[derive(Debug)]
pub(crate) struct Encoder {
    gradients: GradientResolver,
    footprints: Vec<Rect>,
}

impl Encoder {
    pub(crate) const fn new(gradient_atlas: SharedGradientAtlas) -> Self {
        Self {
            gradients: GradientResolver::new(gradient_atlas),
            footprints: Vec::new(),
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
        plan: BuildPlan<'_>,
        out: &mut impl PaintSink,
    ) {
        let BuildPlan {
            render: plan,
            panes: tickets,
        } = plan;
        let viewport = scene.stamp.display.logical_rect();
        let now = scene.stamp.time;
        // Matches the backend's padded physical scissor; both derive from `RenderPlan::AA_PADDING`.
        let damage_cull_margin = RenderPlan::cull_margin(scene.stamp.display.scale_factor());
        // A pane that captures reads everything painted under its footprint before it.
        self.footprints.clear();
        let damage_filter = match &plan.damage {
            Damage::Partial(damage) => {
                self.footprints.extend(
                    scene
                        .panes()
                        .zip(tickets)
                        .filter(|(_, ticket)| ticket.is_some_and(|ticket| ticket.captures))
                        .map(|(pane, _)| pane.footprint),
                );
                Some(CullRegion {
                    damage: &damage.region,
                    footprints: &self.footprints,
                })
            }
            Damage::Full => None,
        };
        let mut gradients = self
            .gradients
            .begin(scene.forest.record_store.gradients.records.as_slice());
        let mut first_pane = 0;
        for (plane, tree) in scene.forest.trees.iter_paint_order() {
            let plane_cascades = &scene.cascade.planes[plane];
            let panes = plane_cascades.panes.as_slice();
            let plane_tickets = &tickets[first_pane..first_pane + panes.len()];
            first_pane += panes.len();
            let mut ctx = PlaneCtx {
                tree,
                layout: &scene.layout[plane],
                cascade_inputs: plane_cascades.cascade_inputs.as_slice(),
                subtree_paint_rects: plane_cascades.subtree_paint_rects.as_slice(),
                gradients: &mut gradients,
                paint_animation_cursor: tree.paint_animations.cursor(),
                gpu_views: scene.gpu_views,
                damage_filter,
                panes,
                tickets: plane_tickets,
                next_pane: 0,
                damage_cull_margin,
                viewport,
                now,
            };
            for root in &tree.roots {
                ctx.encode_node(root.first_node, out);
            }
        }

        debug_assert_eq!(first_pane, tickets.len(), "a ticket for each pane");

        #[cfg(debug_assertions)]
        collision_overlay::emit(scene, out);
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::internals::paint_capture::PaintCapture;
    use crate::internals::scratch_report;
    use crate::renderer::frontend::encoder::Encoder;
    use crate::renderer::frontend::{BuildPlan, FrameScene};
    use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
    use crate::renderer::pane_captures::{PaneCaptures, PaneDamage};
    use crate::renderer::render_plan::RenderPlan;

    impl Encoder {
        pub(crate) fn scratch_bytes(&self) -> usize {
            let Self {
                gradients,
                footprints,
            } = self;
            gradients.scratch_bytes() + scratch_report::vec_bytes(footprints)
        }
    }

    /// Encode `scene` under `plan` into a capture, every pane capturing, as a fresh window's do.
    pub(crate) fn encode(
        scene: FrameScene<'_>,
        gradient_atlas: &SharedGradientAtlas,
        plan: RenderPlan,
    ) -> PaintCapture {
        let mut encoder = Encoder::new(gradient_atlas.clone());
        let mut ledger = PaneCaptures::default();
        ledger.decide(scene.panes(), scene.stamp.display, PaneDamage::Whole);
        let mut recorded = PaintCapture::default();
        encoder.encode(
            &scene,
            BuildPlan {
                render: plan,
                panes: ledger.tickets(),
            },
            &mut recorded,
        );
        recorded
    }
}

#[cfg(test)]
mod tests;
