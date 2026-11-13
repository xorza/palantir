//! The encode pass: walk the cascaded scene into paint calls, one plane at a time ([`plane_ctx`]);
//! [`gradient_resolver`] resolves each gradient once per frame.

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
use crate::renderer::repaint_scissors;

/// The encode pass's retained state: the per-frame gradient memo, and the footprints a partial
/// frame encodes, kept for their capacity.
#[derive(Debug, Default)]
pub(crate) struct Encoder {
    gradients: GradientResolver,
    footprints: Vec<Rect>,
}

impl Encoder {
    /// Walks every tree in paint order, emitting logical-px paint commands into `out`.
    /// `Damage::Partial` culls subtrees whose paint rect intersects no damage rect or captured
    /// backdrop footprint; callers skip the call when nothing is damaged.
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
            backdrops: tickets,
            rows,
        } = plan;
        let viewport = scene.stamp.display.logical_rect();
        let now = scene.stamp.time;
        // Matches the backend's padded physical scissor; both derive from `AA_PADDING_PX`.
        let damage_cull_margin = repaint_scissors::cull_margin(scene.stamp.display.scale_factor());
        // A backdrop that captures reads everything painted under its footprint before it.
        self.footprints.clear();
        let damage_filter = match &plan.damage {
            Damage::Partial(damage) => {
                self.footprints.extend(
                    scene
                        .backdrops()
                        .zip(tickets)
                        .filter(|(_, ticket)| ticket.is_some_and(|ticket| ticket.captures))
                        .map(|(backdrop, _)| backdrop.footprint),
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
            .begin(scene.forest.record_store.gradients.records.as_slice(), rows);
        let mut first_backdrop = 0;
        for (plane, tree) in scene.forest.trees.iter_paint_order() {
            let plane_cascades = &scene.cascade.planes[plane];
            let backdrops = plane_cascades.backdrops.as_slice();
            let plane_tickets = &tickets[first_backdrop..first_backdrop + backdrops.len()];
            first_backdrop += backdrops.len();
            let mut ctx = PlaneCtx {
                tree,
                layout: &scene.layout[plane],
                cascade_inputs: plane_cascades.cascade_inputs.as_slice(),
                subtree_paint_rects: plane_cascades.subtree_paint_rects.as_slice(),
                gradients: &mut gradients,
                paint_animation_cursor: tree.paint_animations.cursor(),
                damage_filter,
                backdrops,
                tickets: plane_tickets,
                next_backdrop: 0,
                damage_cull_margin,
                viewport,
                now,
            };
            for root in &tree.roots {
                ctx.encode_node(root.first_node, out);
            }
        }

        debug_assert_eq!(first_backdrop, tickets.len(), "a ticket for each backdrop");

        collision_overlay::emit(scene, out);
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::internals::paint_capture::PaintCapture;
    use crate::internals::scratch_report;
    use crate::renderer::backdrop_captures::{BackdropCaptures, BackdropDamage};
    use crate::renderer::frontend::encoder::Encoder;
    use crate::renderer::frontend::{BuildPlan, FrameScene};
    use crate::renderer::render_plan::RenderPlan;
    use crate::renderer::row_table::RowTable;

    impl Encoder {
        pub(crate) fn scratch_bytes(&self) -> usize {
            let Self {
                gradients,
                footprints,
            } = self;
            gradients.scratch_bytes() + scratch_report::vec_bytes(footprints)
        }

        /// Encode `scene` under `plan` into a capture, its gradients registered in `rows`, every
        /// backdrop capturing, as a fresh window's do.
        pub(crate) fn capture(
            &mut self,
            scene: FrameScene<'_>,
            plan: RenderPlan,
            rows: &mut RowTable,
        ) -> PaintCapture {
            let mut ledger = BackdropCaptures::default();
            ledger.decide(
                scene.backdrops(),
                scene.stamp.display,
                BackdropDamage::Whole,
            );
            let mut recorded = PaintCapture::default();
            self.encode(
                &scene,
                BuildPlan {
                    render: plan,
                    backdrops: ledger.tickets(),
                    rows,
                },
                &mut recorded,
            );
            recorded
        }
    }
}

#[cfg(test)]
mod tests;
