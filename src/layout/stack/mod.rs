//! Single-axis stack layout — measure, arrange and intrinsic for a panel
//! whose children run along one [`Axis`].
//!
//! Non-`Fill` children measure first. What is left of the main axis after
//! them and the gaps goes through [`FillItem::distribute`], the same
//! `[floor, cap]`-clamped weighted split the grid resolves its Fill tracks
//! with.

use crate::layout::axis::Axis;
use crate::layout::axis_placement::AxisPlacement;
use crate::layout::driver::LayoutDriver;
use crate::layout::engine::LayoutEngine;
use crate::layout::fill_item::FillItem;
use crate::layout::intrinsic::{IntrinsicQuery, IntrinsicRange, LenReq};
use crate::layout::justify_offsets::JustifyOffsets;
use crate::layout::pass::LayoutPass;
use crate::primitives::interned_text::InternedText;
use crate::primitives::{rect::Rect, size::Size};
use crate::scene::tree::Tree;
use crate::scene::tree::node_id::NodeId;

/// Flat depth-shared buffer for the Fill distribution. Layout is the
/// same as `WrapScratch.pool`: each invocation pushes its entries,
/// uses the resulting slice, truncates on exit so nested stacks
/// reuse the tail capacity. Allocation-free in steady state.
#[derive(Debug, Default)]
pub(crate) struct StackScratch {
    pool: Vec<FillItem<NodeId>>,
}

impl StackScratch {
    /// Entries this depth pushed, as a mutable slice. `start` is the
    /// pool length the caller captured on entry.
    #[inline]
    fn from(&mut self, start: usize) -> &mut [FillItem<NodeId>] {
        &mut self.pool[start..]
    }
}

#[derive(Debug)]
struct StackPlan {
    sum_non_fill_main: f32,
    count: usize,
    total_gap: f32,
    fill_start: usize,
}

fn build_stack_plan(
    pass: &mut LayoutPass<'_>,
    node: NodeId,
    axis: Axis,
    gap: f32,
    mut non_fill_main: impl FnMut(&mut LayoutPass<'_>, NodeId) -> f32,
    mut fill_floor: impl FnMut(&mut LayoutPass<'_>, NodeId) -> f32,
) -> StackPlan {
    let tree = pass.tree;
    let layouts = tree.records.layout();
    let fill_start = pass.stack_scratch_mut().pool.len();
    let mut sum_non_fill_main = 0.0f32;
    let mut count = 0usize;
    for c in tree.active_children(node) {
        count += 1;
        let child_layout = layouts[c.idx()];
        if let Some(weight) = axis.main_sizing(child_layout.size).fill_weight() {
            // Floor source depends on the phase the caller is in:
            // `measure` passes the child's `intrinsic(MinContent)` (its
            // largest non-shrinkable descendant), `arrange` its measured
            // `desired.main`. Arrange's is never the lower of the two,
            // since `AxisCtx::resolve` floors `desired` by `intrinsic_min`.
            let floor = fill_floor(pass, c);
            let cap = axis.main(tree.bounds(c).max_size) + axis.spacing(child_layout.margin);
            pass.stack_scratch_mut()
                .pool
                .push(FillItem::new(c, weight, floor, cap));
        } else {
            sum_non_fill_main += non_fill_main(pass, c);
        }
    }
    StackPlan {
        sum_non_fill_main,
        count,
        total_gap: gap * count.saturating_sub(1) as f32,
        fill_start,
    }
}

#[derive(Debug)]
pub(super) struct Stack;

impl LayoutDriver for Stack {
    type Payload = Axis;

    const ARRANGE_DEPENDS_ONLY_ON_SLOT: bool = true;

    fn measure(
        pass: &mut LayoutPass<'_>,
        node: NodeId,
        axis: Self::Payload,
        inner_avail: Size,
    ) -> Size {
        let tree = pass.tree;
        let gap = tree.panel(node).gaps.gap();
        let cross_avail = axis.cross(inner_avail);

        // Pass 1: measure non-Fill children with the stack's committed
        // cross *and* its committed main extent. This is *height-given-width*
        // (or width-given-height): the child shapes/wraps under the finite
        // cross and reports the resulting main-axis size.
        //
        // `main_avail` is the stack's own main extent — `resolve_sizing` has
        // already clamped it to the stack's `Fixed`/`max_size`/inherited
        // bound. When the stack is unbounded on its main axis it's `INF`
        // (the common Hug-in-Hug case: children report their natural main
        // size and the stack grows to fit). When the stack *is* bounded, the
        // bound flows down — so a `max_size` on any ancestor constrains its
        // descendants (CSS `max-height` semantics), and content that wraps or
        // scrolls against the main axis respects it instead of overrunning a
        // box the cap only shrank. Children still clamp at arrange; a rigid
        // child whose content exceeds the bound overflows, same as on the
        // cross axis.
        let main_avail = axis.main(inner_avail);
        let main_finite = main_avail.is_finite();
        let mut max_cross = 0.0f32;
        let StackPlan {
            sum_non_fill_main,
            total_gap,
            fill_start,
            ..
        } = build_stack_plan(
            pass,
            node,
            axis,
            gap,
            |pass, c| {
                let d = pass.measure(c, axis.compose_size(main_avail, cross_avail));
                max_cross = max_cross.max(axis.cross(d));
                axis.main(d)
            },
            |pass, c| {
                if main_finite {
                    pass.intrinsic(c, axis, LenReq::MinContent)
                } else {
                    0.0
                }
            },
        );

        // Pass 2: measure Fill children against their share of what is
        // left. The `MinContent` floor is what stops a sibling with rigid
        // descendants (Fixed widget, longest-unbreakable-word) being
        // squeezed past its content — the other Fill siblings absorb the
        // squeeze instead. Without it, Fixed children overflow visibly
        // when the parent is narrow even though shrinkable siblings still
        // have room to give.
        //
        // On a Hug stack (INF main) there is nothing to divide — every
        // Fill child measures at INF main and reports its natural width.
        //
        // Soundness: the `axis.main(inner_avail)` we use as the budget here
        // must equal the `axis.main(inner.size)` the matching `arrange` call
        // sees, otherwise wrap text in Fill children shapes against the wrong
        // width. It does, because the Stack's outer main size is a
        // deterministic function of (its own `Sizing` + parent-supplied
        // `available`) via `AxisCtx::resolve`, and the parent passes the
        // same `available` to `measure` that determines its arranged outer
        // size. Any future driver that clamps a child's slot
        // *between* its own measure and arrange would break this.
        if main_finite {
            let leftover = (main_avail - sum_non_fill_main - total_gap).max(0.0);
            FillItem::distribute(pass.stack_scratch_mut().from(fill_start), leftover);
        }

        // Snapshot the pool end because recursive measurement may append entries
        // for nested stacks.
        let fill_end = pass.stack_scratch_mut().pool.len();
        let mut fill_main = 0.0f32;
        for i in fill_start..fill_end {
            let entry = pass.stack_scratch_mut().pool[i];
            let fill_avail = if main_finite {
                entry.size
            } else {
                f32::INFINITY
            };
            let desired = pass.measure(entry.key, axis.compose_size(fill_avail, cross_avail));
            fill_main += axis.main(desired);
            max_cross = max_cross.max(axis.cross(desired));
        }
        pass.stack_scratch_mut().pool.truncate(fill_start);

        axis.compose_size(sum_non_fill_main + fill_main + total_gap, max_cross)
    }

    fn arrange(pass: &mut LayoutPass<'_>, node: NodeId, axis: Self::Payload, inner: Rect) {
        let tree = pass.tree;
        let panel = tree.panel(node);
        let (gap, justify, parent_child_align) =
            (panel.gaps.gap(), panel.justify, panel.child_align);

        // WPF Stretch semantics: `Fill` (the Stretch hint) reports content
        // size at measure-time (so a Hug ancestor doesn't balloon to its
        // grandparent's allocation), then expands at *arrange* to its share
        // of the slot. Re-run the floor-aware freeze loop here against
        // `inner.main` (the slot we actually got) so Fill children stretch
        // to fill leftover. Without this, Fill children would arrange at
        // their measured content size and the parent's leftover would just
        // dead-space.
        let layouts = tree.records.layout();
        // Shares the count / weight / gap accounting with `measure`; the
        // closure supplies the per-phase main source — here the cached
        // `desired.main` (Fill children's content size, since the
        // `AxisCtx::resolve` change pins Fill at content).
        let StackPlan {
            sum_non_fill_main,
            count,
            total_gap,
            fill_start,
        } = build_stack_plan(
            pass,
            node,
            axis,
            gap,
            |pass, c| axis.main(pass.desired(c)),
            |pass, c| axis.main(pass.desired(c)),
        );
        // The distribution mirrors `measure`: a child whose share falls
        // outside `[floor, cap]` takes the bound, then the rest re-share.
        let main_total = axis.main(inner.size);
        let cross = axis.cross(inner.size);
        let leftover_for_fill = (main_total - sum_non_fill_main - total_gap).max(0.0);
        FillItem::distribute(pass.stack_scratch_mut().from(fill_start), leftover_for_fill);
        // The sum we report to `justify` is the post-redistribute total —
        // i.e., what the children will *actually* occupy after arrange.
        let sum_main_arranged = sum_non_fill_main
            + pass
                .stack_scratch_mut()
                .from(fill_start)
                .iter()
                .map(|entry| entry.size)
                .sum::<f32>();
        let leftover_for_justify = (main_total - sum_main_arranged - total_gap).max(0.0);

        // `justify` distributes any *remaining* main-axis slack. With Fill
        // children that hit their cap (or with zero leftover) we may still
        // have free pixels — justify them out.
        let JustifyOffsets {
            start: start_offset,
            gap: effective_gap,
        } = JustifyOffsets::new(justify, leftover_for_justify, gap, count);

        let cross_min = axis.cross_v(inner.min);
        let mut cursor = axis.main_v(inner.min) + start_offset;
        let mut first = true;
        let mut fill_cursor = fill_start;

        for child in tree.children(node) {
            let c = child.id;
            if child.visibility.is_collapsed() {
                pass.zero_subtree(c, axis.compose_point(cursor, cross_min));
                continue;
            }
            let i = c.idx();
            let s = layouts[i];
            let d = pass.desired(c);
            if !first {
                cursor += effective_gap;
            }
            first = false;

            let main_size = if axis.main_sizing(s.size).fill_weight().is_some() {
                let alloc = pass.stack_scratch_mut().pool[fill_cursor].size;
                fill_cursor += 1;
                alloc
            } else {
                axis.main(d)
            };

            let bounds = tree.bounds(c);
            let cross_p = AxisPlacement::cross(axis, &s, bounds, parent_child_align, d, cross);

            let child_rect =
                axis.compose_rect(cursor, cross_min + cross_p.offset, main_size, cross_p.size);
            pass.arrange(c, child_rect);
            cursor += main_size;
        }
        pass.stack_scratch_mut().pool.truncate(fill_start);
    }

    /// Intrinsic size of a stack on `query_axis`. When the query
    /// axis matches the stack's `main_axis`, sum children's intrinsic on
    /// that axis plus gaps; otherwise (cross axis), max over children.
    fn intrinsic(
        layout: &mut LayoutEngine,
        tree: &Tree,
        node: NodeId,
        main_axis: Self::Payload,
        query_axis: Axis,
        query: IntrinsicQuery,
        interned_text: &InternedText<'_>,
    ) -> IntrinsicRange {
        if main_axis != query_axis {
            return query.children_max_at_origin(layout, tree, node, query_axis, interned_text);
        }
        let mut range = IntrinsicRange::ZERO;
        let mut count = 0_usize;
        for c in tree.active_children(node) {
            let child = query.child(layout, tree, c, query_axis, interned_text);
            for (req, slot) in range.requested(query) {
                *slot += child.get(req);
            }
            count += 1;
        }
        let gaps = tree.panel(node).gaps.gap() * count.saturating_sub(1) as f32;
        for (_, slot) in range.requested(query) {
            *slot += gaps;
        }
        range
    }
}

#[cfg(test)]
mod tests;
