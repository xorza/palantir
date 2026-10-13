//! Single-axis stack layout (measure, arrange, intrinsic) for a panel whose
//! children run along one [`Axis`]. The main axis is shared through `axis_share`,
//! as in the grid: `Fill` floors are set aside, the others give way from what they
//! want toward their floors, and the `Fill` children divide the rest.
//!
//! **Width in, height out.** Text wraps to the width it is given, so on a
//! horizontal main axis widths are shared before the children measure and each
//! measures at its share. A height is what a child measures *to*, so on a vertical
//! axis children measure against the whole extent and heights are shared at
//! arrange.

use crate::primitives::layout::offer;

use crate::layout::axis_share;
use crate::layout::drivers::LayoutDriver;
use crate::layout::drivers::stack::stack_scratch::StackScratch;
use crate::layout::fill_item::{FillDemand, FillItem};
use crate::layout::hug_item::HugItem;
use crate::layout::intrinsic::intrinsic_pass::IntrinsicPass;
use crate::layout::intrinsic::intrinsic_query::IntrinsicQuery;
use crate::layout::intrinsic::intrinsic_range::IntrinsicRange;
use crate::layout::intrinsic::len_req::LenReq;
use crate::layout::justify_offsets::{JustifyOffsets, JustifyRow};
use crate::layout::measured::Measured;
use crate::layout::pass::LayoutPass;
use crate::layout::placed_child::Placed;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::axis::{Axis, AxisExtent};
use crate::primitives::math::num::F32Px;
use crate::scene::tree::node_id::NodeId;
use glam::BVec2;

pub(crate) mod stack_scratch;

#[derive(Debug)]
struct StackPlan {
    sum_non_fill_main: f32,
    count: usize,
    total_gap: f32,
    fill_start: usize,
    hug_start: usize,
}

#[derive(Debug)]
struct WidthShare {
    start: usize,
    squeezed: bool,
    stable_from: f32,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Stack {
    node: NodeId,
    axis: Axis,
    /// The axes with no give: a scroll's panned axes, where content takes what
    /// it measured to and a Fill child only grows.
    rigid: BVec2,
}

impl Stack {
    pub(super) const fn new(node: NodeId, axis: Axis) -> Self {
        Self {
            node,
            axis,
            rigid: BVec2::FALSE,
        }
    }

    /// This stack with no give on the axes `rigid` sets.
    pub(super) const fn rigid_on(self, rigid: BVec2) -> Self {
        Self { rigid, ..self }
    }

    /// Walk the active children once: push a [`FillItem`] per `Fill` child and hand
    /// every other to `non_fill`, which pushes its [`HugItem`] and returns what it
    /// wants.
    fn plan(
        self,
        pass: &mut LayoutPass<'_>,
        mut non_fill: impl FnMut(&mut LayoutPass<'_>, NodeId) -> f32,
        mut fill_floor: impl FnMut(&mut LayoutPass<'_>, NodeId) -> f32,
    ) -> StackPlan {
        let Self { node, axis, .. } = self;
        let tree = pass.tree;
        let layouts = tree.records.layout();
        let fill_start = pass.stack_scratch_mut().fill.mark();
        let hug_start = pass.stack_scratch_mut().hug.mark();
        let mut sum_non_fill_main = 0.0f32;
        let mut count = 0usize;
        for c in tree.active_children(node) {
            count += 1;
            let child_layout = layouts[c.idx()];
            if let Some(weight) = axis.main_sizing(child_layout.size).fill_weight() {
                // The floor source depends on the phase: `measure` passes
                // `intrinsic(MinContent)`, `arrange` its measured floor.
                let floor = fill_floor(pass, c);
                let cap = axis.main(tree.bounds(c).max_size) + axis.spacing(child_layout.margin);
                pass.stack_scratch_mut()
                    .fill
                    .push(FillItem::new(c, FillDemand { weight, floor, cap }));
            } else {
                sum_non_fill_main += non_fill(pass, c);
            }
        }
        StackPlan {
            sum_non_fill_main,
            count,
            total_gap: tree.panel(node).gaps.gap().gaps_between(count),
            fill_start,
            hug_start,
        }
    }

    /// Share a horizontal main axis before the children measure, from each child's
    /// intrinsic range (wants = max-content, floor = min-content). Leaves the pools as
    /// found.
    fn share_widths(self, pass: &mut LayoutPass<'_>, main_avail: f32) -> WidthShare {
        debug_assert_eq!(self.axis, Axis::X, "only a width is shared before measure");
        let StackPlan {
            total_gap,
            fill_start,
            hug_start,
            ..
        } = self.plan(
            pass,
            |pass, c| {
                let range = pass.intrinsic_range(c, Axis::X);
                let wants = range.max.max(range.min);
                pass.stack_scratch_mut()
                    .hug
                    .push(HugItem::new(c, range.min, wants));
                wants
            },
            |pass, c| pass.intrinsic(c, Axis::X, LenReq::MinContent),
        );
        let StackScratch { fill, hug } = pass.stack_scratch_mut();
        let shares_from = axis_share::solve(
            hug.since(hug_start),
            fill.since(fill_start),
            (main_avail - total_gap).max(0.0),
        );
        fill.truncate(fill_start);
        WidthShare {
            start: hug_start,
            squeezed: shares_from == offer::AT_OFFER_ONLY,
            stable_from: if shares_from > 0.0 {
                shares_from + total_gap
            } else {
                0.0
            },
        }
    }
}

impl LayoutDriver for Stack {
    const ARRANGE_DEPENDS_ONLY_ON_SLOT: bool = true;

    fn measure(self, pass: &mut LayoutPass<'_>, inner_avail: Size) -> Measured {
        let axis = self.axis;
        let cross_avail = axis.cross(inner_avail);

        // Pass 1: measure non-Fill children at the stack's committed cross and main
        // extent. `main_avail` is `INF` when unbounded (Hug-in-Hug); a bound flows
        // down, so an ancestor's `max_size` constrains descendants (CSS
        // `max-height`).
        let main_avail = axis.main(inner_avail);
        let main_finite = main_avail.is_finite();
        // Width in: a squeezing width share hands each child its share; otherwise
        // every child measures at the whole width.
        let widths = (axis == Axis::X && main_finite).then(|| self.share_widths(pass, main_avail));
        let mut width_cursor = widths.as_ref().map_or(0, |w| w.start);
        let mut max_cross = 0.0f32;
        let mut floor_main = 0.0f32;
        let mut floor_cross = 0.0f32;
        // Each child is offered the stack's main and cross, so it holds while those
        // stay past its range.
        let mut stable_main = 0.0f32;
        let mut stable_cross = 0.0f32;
        let StackPlan {
            sum_non_fill_main,
            total_gap,
            fill_start,
            hug_start,
            ..
        } = self.plan(
            pass,
            |pass, c| {
                let offer = match &widths {
                    Some(w) if w.squeezed => {
                        let share = pass.stack_scratch_mut().hug.at(width_cursor).size;
                        width_cursor += 1;
                        share
                    }
                    _ => main_avail,
                };
                let d = pass.measure(c, axis.compose_size(offer, cross_avail));
                let wants = axis.main(d.size);
                // What arrange will let this child give way to, so the Fill shares
                // below match.
                let gives_to = axis.main(Placed::of(d.size, d.floor).floor);
                pass.stack_scratch_mut()
                    .hug
                    .push(HugItem::new(c, gives_to, wants));
                max_cross = max_cross.max(axis.cross(d.size));
                floor_main += axis.main(d.floor);
                floor_cross = floor_cross.max(axis.cross(d.floor));
                stable_main = stable_main.max(axis.main(d.stable_from));
                stable_cross = stable_cross.max(axis.cross(d.stable_from));
                wants
            },
            |pass, c| {
                if main_finite {
                    pass.intrinsic(c, axis, LenReq::MinContent)
                } else {
                    0.0
                }
            },
        );

        // Share the main axis as arrange will, so each Fill child measures at its
        // arranged share (floors first, so a rigid or wrapping sibling never
        // squeezes it below content). Soundness: `axis.main(inner_avail)` must
        // equal the `axis.main(inner)` arrange sees, or wrapped Fill text shapes at
        // the wrong width; it does, as the outer main size is a deterministic
        // function of `Sizing` and the parent's `available`.
        let fill_end = pass.stack_scratch_mut().fill.mark();
        if main_finite && fill_end > fill_start {
            let StackScratch { fill, hug } = pass.stack_scratch_mut();
            axis_share::solve(
                hug.since(hug_start),
                fill.since(fill_start),
                (main_avail - total_gap).max(0.0),
            );
        }

        let mut fill_main = 0.0f32;
        for i in fill_start..fill_end {
            let entry = pass.stack_scratch_mut().fill.at(i);
            let fill_avail = if main_finite {
                entry.size
            } else {
                f32::INFINITY
            };
            let desired = pass.measure(entry.key, axis.compose_size(fill_avail, cross_avail));
            fill_main += axis.main(desired.size);
            max_cross = max_cross.max(axis.cross(desired.size));
            floor_main += axis.main(desired.floor);
            floor_cross = floor_cross.max(axis.cross(desired.floor));
            stable_cross = stable_cross.max(axis.cross(desired.stable_from));
        }
        let scratch = pass.stack_scratch_mut();
        scratch.fill.truncate(fill_start);
        scratch
            .hug
            .truncate(widths.as_ref().map_or(hug_start, |w| w.start));
        // A Fill or squeezed child's share moves with every change to the axis;
        // unsqueezed widths hold while the children still fit.
        if fill_end > fill_start {
            stable_main = offer::AT_OFFER_ONLY;
        }
        if let Some(w) = &widths {
            stable_main = stable_main.max(w.stable_from);
        }

        Measured {
            size: axis.compose_size(sum_non_fill_main + fill_main + total_gap, max_cross),
            floor: axis.compose_size(floor_main + total_gap, floor_cross),
            stable_from: axis.compose_size(stable_main, stable_cross),
        }
    }

    fn arrange(self, pass: &mut LayoutPass<'_>, inner: Size) {
        let Self { node, axis, rigid } = self;
        let tree = pass.tree;
        let panel = tree.panel(node);
        let (gap, justify, parent_child_align) =
            (panel.gaps.gap(), panel.justify, panel.child_align);

        // WPF Stretch: `Fill` reports content size at measure (so a Hug ancestor
        // does not balloon), then expands at *arrange*.
        let measured = |pass: &LayoutPass<'_>, c: NodeId| pass.placed(c).measured.rigid_on(rigid);
        let StackPlan {
            count,
            total_gap,
            fill_start,
            hug_start,
            ..
        } = self.plan(
            pass,
            |pass, c| {
                let p = measured(pass, c);
                let wants = axis.main(p.desired);
                pass.stack_scratch_mut()
                    .hug
                    .push(HugItem::new(c, axis.main(p.floor), wants));
                wants
            },
            |pass, c| axis.main(measured(pass, c).floor),
        );
        let main_total = axis.main(inner);
        let cross = axis.cross(inner);
        let StackScratch { fill, hug } = pass.stack_scratch_mut();
        axis_share::solve(
            hug.since(hug_start),
            fill.since(fill_start),
            (main_total - total_gap).max(0.0),
        );
        let sum_main_arranged = hug
            .since(hug_start)
            .iter()
            .map(|item| item.size)
            .sum::<f32>()
            + fill
                .since(fill_start)
                .iter()
                .map(|item| item.size)
                .sum::<f32>();
        let leftover_for_justify = (main_total - sum_main_arranged - total_gap).max(0.0);

        // `justify` distributes remaining slack, which may remain even with capped
        // Fill children.
        let JustifyOffsets {
            start: start_offset,
            gap: effective_gap,
        } = JustifyOffsets::new(
            justify,
            JustifyRow {
                leftover: leftover_for_justify,
                gap,
                count,
            },
        );

        let cross_min = 0.0;
        let mut cursor = start_offset;
        let mut first = true;
        let mut fill_cursor = fill_start;
        let mut hug_cursor = hug_start;

        for child in tree.children(node) {
            let c = child.id;
            if child.visibility.is_collapsed() {
                pass.zero_subtree(c, axis.compose_point(cursor, cross_min));
                continue;
            }
            if !first {
                cursor += effective_gap;
            }
            first = false;

            let placed_child = pass.placed(c).rigid_on(rigid);
            let main_size = if axis
                .main_sizing(placed_child.layout.size)
                .fill_weight()
                .is_some()
            {
                let alloc = pass.stack_scratch_mut().fill.at(fill_cursor).size;
                fill_cursor += 1;
                alloc
            } else {
                let share = pass.stack_scratch_mut().hug.at(hug_cursor).size;
                hug_cursor += 1;
                share
            };

            let cross_p = placed_child.cross(axis, parent_child_align, cross);

            let child_rect = axis.compose_rect(
                AxisExtent::new(cursor, main_size),
                AxisExtent::new(cross_min + cross_p.offset, cross_p.size),
            );
            pass.arrange(c, child_rect);
            cursor += main_size;
        }
        let scratch = pass.stack_scratch_mut();
        scratch.fill.truncate(fill_start);
        scratch.hug.truncate(hug_start);
    }

    fn intrinsic(self, pass: &mut IntrinsicPass<'_>, query: IntrinsicQuery) -> IntrinsicRange {
        if self.axis != query.axis() {
            return pass.children_max_at_origin(self.node, query);
        }
        let tree = pass.tree();
        let mut range = IntrinsicRange::ZERO;
        let mut count = 0_usize;
        for c in tree.active_children(self.node) {
            let child = pass.query(c, query);
            for (req, slot) in range.requested(query) {
                *slot += child.get(req);
            }
            count += 1;
        }
        let gaps = tree.panel(self.node).gaps.gap().gaps_between(count);
        for (_, slot) in range.requested(query) {
            *slot += gaps;
        }
        range
    }
}

#[cfg(test)]
mod tests;
