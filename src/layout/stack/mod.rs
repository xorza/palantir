//! Single-axis stack layout — measure, arrange and intrinsic for a panel
//! whose children run along one [`Axis`].
//!
//! The main axis is shared through `axis_share`, the solve the grid
//! sizes its tracks with: the `Fill` children's floors are set aside, the
//! other children share what is left — each giving way from what it
//! wants toward its floor when they do not all fit — and the `Fill`
//! children divide the rest.
//!
//! **Width in, height out**, as the grid splits its columns and rows.
//! Text wraps to the width it is given, so on a horizontal main axis the
//! widths are shared before the children measure, from their intrinsic
//! ranges, and each child measures at its share. A height is what a
//! child measures *to*, so on a vertical main axis the children measure
//! against the whole extent and the heights are shared at arrange, from
//! what each measured to and its floor.

use crate::layout::axis::Axis;
use crate::layout::axis_placement::{AxisPlacement, Placed};
use crate::layout::axis_share;
use crate::layout::driver::LayoutDriver;
use crate::layout::engine::LayoutEngine;
use crate::layout::fill_item::FillItem;
use crate::layout::hug_item::HugItem;
use crate::layout::intrinsic::intrinsic_query::IntrinsicQuery;
use crate::layout::intrinsic::intrinsic_range::IntrinsicRange;
use crate::layout::intrinsic::len_req::LenReq;
use crate::layout::justify_offsets::JustifyOffsets;
use crate::layout::measured::Measured;
use crate::layout::pass::LayoutPass;
use crate::layout::stack::stack_scratch::StackScratch;
use crate::primitives::interned_text::InternedText;
use crate::primitives::num::F32Px;
use crate::primitives::{rect::Rect, size::Size};
use crate::scene::tree::Tree;
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

/// Walk the active children once: push a [`FillItem`] for each `Fill`
/// child, and hand every other child to `non_fill`, which pushes its
/// [`HugItem`] and returns what it wants.
fn build_stack_plan(
    pass: &mut LayoutPass<'_>,
    node: NodeId,
    axis: Axis,
    gap: f32,
    mut non_fill: impl FnMut(&mut LayoutPass<'_>, NodeId) -> f32,
    mut fill_floor: impl FnMut(&mut LayoutPass<'_>, NodeId) -> f32,
) -> StackPlan {
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
            // Floor source depends on the phase the caller is in:
            // `measure` passes the child's `intrinsic(MinContent)` (its
            // largest non-shrinkable descendant), since a Fill child has
            // not measured yet; `arrange` its measured floor.
            let floor = fill_floor(pass, c);
            let cap = axis.main(tree.bounds(c).max_size) + axis.spacing(child_layout.margin);
            pass.stack_scratch_mut()
                .fill
                .push(FillItem::new(c, weight, floor, cap));
        } else {
            sum_non_fill_main += non_fill(pass, c);
        }
    }
    StackPlan {
        sum_non_fill_main,
        count,
        total_gap: gap.gaps_between(count),
        fill_start,
        hug_start,
    }
}

/// What a stack's width share decided before its children measured.
#[derive(Debug)]
struct WidthShare {
    /// Where the shares sit in the Hug pool, one per non-`Fill` child in
    /// child order.
    start: usize,
    /// Whether the children did not all fit at what they want, so each
    /// measures at its share rather than at the whole width.
    squeezed: bool,
    /// The least width from which the shares hold — see
    /// [`Measured::stable_from`].
    stable_from: f32,
}

/// Share a horizontal main axis of `main_avail` before the children
/// measure, from each one's intrinsic range: what it wants is its
/// max-content width, what it cannot go below its min-content width.
/// Leaves the shares in the Hug pool and the Fill pool as it found it.
fn share_widths(pass: &mut LayoutPass<'_>, node: NodeId, gap: f32, main_avail: f32) -> WidthShare {
    let StackPlan {
        total_gap,
        fill_start,
        hug_start,
        ..
    } = build_stack_plan(
        pass,
        node,
        Axis::X,
        gap,
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
        squeezed: shares_from == Measured::AT_OFFER_ONLY,
        stable_from: if shares_from > 0.0 {
            shares_from + total_gap
        } else {
            0.0
        },
    }
}

#[derive(Debug)]
pub(super) struct Stack;

impl Stack {
    /// [`LayoutDriver::arrange`], with the children given no give on the
    /// axes `rigid` sets — a scroll's panned axes, where its content takes
    /// what it measured to however small the viewport, and a Fill child
    /// only grows to fill it.
    pub(super) fn arrange_in(
        pass: &mut LayoutPass<'_>,
        node: NodeId,
        axis: Axis,
        inner: Rect,
        rigid: BVec2,
    ) {
        let tree = pass.tree;
        let panel = tree.panel(node);
        let (gap, justify, parent_child_align) =
            (panel.gaps.gap(), panel.justify, panel.child_align);

        // WPF Stretch semantics: `Fill` (the Stretch hint) reports content
        // size at measure-time (so a Hug ancestor doesn't balloon to its
        // grandparent's allocation), then expands at *arrange* to its share
        // of the slot. The other children share what is left as measure
        // shared it, from what each measured to and its floor — which on a
        // vertical axis is where a child that wants more than its siblings
        // leave gives way (height out; see the module doc).
        let layouts = tree.records.layout();
        let placed = |pass: &LayoutPass<'_>, c: NodeId| pass.placed(c).rigid_on(rigid);
        let StackPlan {
            count,
            total_gap,
            fill_start,
            hug_start,
            ..
        } = build_stack_plan(
            pass,
            node,
            axis,
            gap,
            |pass, c| {
                let p = placed(pass, c);
                let wants = axis.main(p.desired);
                pass.stack_scratch_mut()
                    .hug
                    .push(HugItem::new(c, axis.main(p.floor), wants));
                wants
            },
            |pass, c| axis.main(placed(pass, c).floor),
        );
        let main_total = axis.main(inner.size);
        let cross = axis.cross(inner.size);
        let StackScratch { fill, hug } = pass.stack_scratch_mut();
        axis_share::solve(
            hug.since(hug_start),
            fill.since(fill_start),
            (main_total - total_gap).max(0.0),
        );
        // The sum we report to `justify` is what the children will
        // *actually* occupy after the share.
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
        let mut hug_cursor = hug_start;

        for child in tree.children(node) {
            let c = child.id;
            if child.visibility.is_collapsed() {
                pass.zero_subtree(c, axis.compose_point(cursor, cross_min));
                continue;
            }
            let i = c.idx();
            let s = layouts[i];
            if !first {
                cursor += effective_gap;
            }
            first = false;

            let main_size = if axis.main_sizing(s.size).fill_weight().is_some() {
                let alloc = pass.stack_scratch_mut().fill.at(fill_cursor).size;
                fill_cursor += 1;
                alloc
            } else {
                let share = pass.stack_scratch_mut().hug.at(hug_cursor).size;
                hug_cursor += 1;
                share
            };

            let bounds = tree.bounds(c);
            let cross_p =
                AxisPlacement::cross(axis, &s, bounds, parent_child_align, placed(pass, c), cross);

            let child_rect =
                axis.compose_rect(cursor, cross_min + cross_p.offset, main_size, cross_p.size);
            pass.arrange(c, child_rect);
            cursor += main_size;
        }
        let scratch = pass.stack_scratch_mut();
        scratch.fill.truncate(fill_start);
        scratch.hug.truncate(hug_start);
    }
}

impl LayoutDriver for Stack {
    type Payload = Axis;

    const ARRANGE_DEPENDS_ONLY_ON_SLOT: bool = true;

    fn measure(
        pass: &mut LayoutPass<'_>,
        node: NodeId,
        axis: Self::Payload,
        inner_avail: Size,
    ) -> Measured {
        let tree = pass.tree;
        let gap = tree.panel(node).gaps.gap();
        let cross_avail = axis.cross(inner_avail);

        // Pass 1: measure non-Fill children with the stack's committed
        // cross *and* its committed main extent. This is *height-given-width*
        // (or width-given-height): the child shapes/wraps under the finite
        // cross and reports the resulting main-axis size.
        //
        // `main_avail` is the stack's own main extent — `AxisSlot::resolve` has
        // already clamped it to the stack's `Fixed`/`max_size`/inherited
        // bound. When the stack is unbounded on its main axis it's `INF`
        // (the common Hug-in-Hug case: children report their natural main
        // size and the stack grows to fit). When the stack *is* bounded, the
        // bound flows down — so a `max_size` on any ancestor constrains its
        // descendants (CSS `max-height` semantics), and content that wraps or
        // scrolls against the main axis respects it instead of overrunning a
        // box the cap only shrank. Arrange then shares the extent among
        // them; a rigid child whose content exceeds the bound overflows,
        // same as on the cross axis.
        let main_avail = axis.main(inner_avail);
        let main_finite = main_avail.is_finite();
        // Width in: see the module doc. A width share that squeezes hands
        // each child its share to measure at; otherwise every child
        // measures at the whole width, as on a vertical axis.
        let widths =
            (axis == Axis::X && main_finite).then(|| share_widths(pass, node, gap, main_avail));
        let mut width_cursor = widths.as_ref().map_or(0, |w| w.start);
        let mut max_cross = 0.0f32;
        // The floor composes like the content: children's floors summed
        // along the pack axis, the widest across it.
        let mut floor_main = 0.0f32;
        let mut floor_cross = 0.0f32;
        // Every child is offered the stack's own main and cross, so each
        // holds as long as those stay past its range.
        let mut stable_main = 0.0f32;
        let mut stable_cross = 0.0f32;
        let StackPlan {
            sum_non_fill_main,
            total_gap,
            fill_start,
            hug_start,
            ..
        } = build_stack_plan(
            pass,
            node,
            axis,
            gap,
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
                // What arrange will let this child give way to, so the
                // Fill shares below are the ones arrange hands out.
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

        // Share the main axis as arrange will, so each Fill child measures
        // at the share it is arranged at: its floor is set aside first, so
        // a rigid or wrapping sibling never squeezes it below its content,
        // and it takes what the others leave. On an unbounded main axis
        // there is nothing to share — every Fill child measures at INF and
        // reports its natural extent.
        //
        // Soundness: the `axis.main(inner_avail)` we use as the budget here
        // must equal the `axis.main(inner.size)` the matching `arrange` call
        // sees, otherwise wrap text in Fill children shapes against the wrong
        // width. It does, because the Stack's outer main size is a
        // deterministic function of (its own `Sizing` + parent-supplied
        // `available`) via `AxisSlot::resolve`, and the parent passes the
        // same `available` to `measure` that determines its arranged outer
        // size.
        //
        // The share sizes the Fill children alone here, so a stack without
        // one has nothing to solve. The pool ends where the walk above left
        // it: every nested measure truncated what it pushed.
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
        // A Fill child is offered its share of the main axis, and a child
        // of a squeezed width share its own share, each of which moves with
        // every change to the axis. Unsqueezed, the widths hold while what
        // the children want still fits.
        if fill_end > fill_start {
            stable_main = Measured::AT_OFFER_ONLY;
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

    fn arrange(pass: &mut LayoutPass<'_>, node: NodeId, axis: Self::Payload, inner: Rect) {
        Self::arrange_in(pass, node, axis, inner, BVec2::FALSE);
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
        let gaps = tree.panel(node).gaps.gap().gaps_between(count);
        for (_, slot) in range.requested(query) {
            *slot += gaps;
        }
        range
    }
}

#[cfg(test)]
mod tests;
