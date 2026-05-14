use super::axis::Axis;
use super::intrinsic::LenReq;
use super::layoutengine::LayoutEngine;
use super::support::{
    JustifyOffsets, children_max_intrinsic, cross_place, justify_offsets, zero_subtree,
};
use crate::forest::tree::{NodeId, Tree};
use crate::layout::Layout;
use crate::layout::types::sizing::Sizing;
use crate::primitives::{rect::Rect, size::Size};
use crate::text::TextShaper;

/// One Fill child as the freeze loop sees it. Pushed onto
/// `LayoutScratch::stack_fill` during measure; popped at the end of
/// the call. `frozen_alloc = Some(v)` means this child has been
/// removed from the active pool and gets exactly `v` main-axis space.
#[derive(Clone, Copy)]
pub(crate) struct FillEntry {
    node: NodeId,
    weight: f32,
    floor: f32,
    cap: f32,
    frozen_alloc: Option<f32>,
}

/// Flat depth-shared buffer for the Fill freeze loop. Layout is the
/// same as `WrapScratch.pool`: each invocation pushes its entries,
/// uses the resulting slice, truncates on exit so nested stacks
/// reuse the tail capacity. Allocation-free in steady state.
#[derive(Default)]
pub(crate) struct StackScratch {
    pub(crate) pool: Vec<FillEntry>,
}

#[profiling::function]
pub(crate) fn measure(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    inner: Size,
    axis: Axis,
    text: &TextShaper,
    out: &mut Layout,
) -> Size {
    let gap = tree.panel(node).gaps.gap();
    let cross_avail = axis.cross(inner);

    // Pass 1: measure non-Fill children at `INF` main with the stack's
    // committed cross. This is *height-given-width* (or width-given-
    // height): the child shapes/wraps under the finite cross and
    // reports the resulting main-axis size. `intrinsic(MaxContent)`
    // would return the unbounded answer, ignoring the cross — wrong
    // for any child whose main depends on cross (Grid with wrapping
    // cells, VStack of wrapping leaves, etc.).
    let mut sum_non_fill_main = 0.0f32;
    let mut total_weight = 0.0f32;
    let mut max_cross = 0.0f32;
    let mut count = 0usize;
    for c in tree.active_children(node) {
        count += 1;
        let l = tree.records.layout()[c.index()];
        if let Sizing::Fill(w) = axis.main_sizing(l.size) {
            total_weight += w;
            continue;
        }
        let d = layout.measure(
            tree,
            c,
            axis.compose_size(f32::INFINITY, cross_avail),
            text,
            out,
        );
        sum_non_fill_main += axis.main(d);
        max_cross = max_cross.max(axis.cross(d));
    }
    let total_gap = gap * count.saturating_sub(1) as f32;

    // Pass 2: measure Fill children with min-content-aware
    // distribution (CSS Flexbox-style). On a finite-main stack, each
    // Fill child gets a target = `leftover * weight / total_weight`,
    // floored at `MinContent` and capped at `max_size`. If any
    // child's floor exceeds its target, freeze that child at its
    // floor and re-divide the remaining leftover among the
    // non-frozen siblings — repeat until stable. This means a sibling
    // with rigid descendants (Fixed widget, longest-unbreakable-word)
    // doesn't get squeezed past its min-content; instead the other
    // FILL siblings absorb the squeeze. Without this freeze loop,
    // Fixed children overflow visibly when the parent is narrow even
    // though shrinkable siblings still have room to give. Converges
    // in ≤ N iterations (every iteration freezes at least one).
    //
    // On a Hug stack (INF main) the freeze loop is a no-op — every
    // Fill child measures at INF main and reports its natural width.
    //
    // Soundness: the `axis.main(inner)` we use as the budget here must
    // equal the `axis.main(inner)` the matching `arrange` call sees,
    // otherwise wrap text in Fill children shapes against the wrong
    // width. It does, because the Stack's outer main size is a
    // deterministic function of (its own `Sizing` + parent-supplied
    // `available`) via `resolve_axis_size`, and the parent passes the
    // same `available` to `measure` that it later derives `slot.size`
    // from for `arrange`. Any future driver that clamps a child's slot
    // *between* its own measure and arrange would break this.
    let mut fill_main = 0.0f32;
    if total_weight > 0.0 {
        let main_finite = axis.main(inner).is_finite();
        if main_finite {
            // Reentrancy-safe flat pool: record pool-end on entry,
            // push entries, slice through `[start..]`, truncate at
            // exit. Nested stacks reuse the tail capacity.
            let pool_start = layout.scratch.stack_fill.pool.len();
            for c in tree.active_children(node) {
                let Sizing::Fill(w) = axis.main_sizing(tree.records.layout()[c.index()].size)
                else {
                    continue;
                };
                let cap = axis.main(tree.size_clamps_of(c).max);
                let floor = layout.intrinsic(tree, c, axis, LenReq::MinContent, text);
                layout.scratch.stack_fill.pool.push(FillEntry {
                    node: c,
                    weight: w,
                    floor,
                    cap,
                    frozen_alloc: None,
                });
            }
            let total_leftover = (axis.main(inner) - sum_non_fill_main - total_gap).max(0.0);
            let mut remaining = total_leftover;
            let mut active_weight = total_weight;
            // Freeze loop: any child whose share-by-weight is below
            // its floor takes its floor and exits the pool.
            loop {
                let mut new_freeze = false;
                if active_weight <= 0.0 {
                    break;
                }
                let entries = &mut layout.scratch.stack_fill.pool[pool_start..];
                for e in entries.iter_mut() {
                    if e.frozen_alloc.is_some() {
                        continue;
                    }
                    let share = (remaining * e.weight / active_weight).min(e.cap);
                    if e.floor > share {
                        let alloc = e.floor.min(e.cap);
                        e.frozen_alloc = Some(alloc);
                        remaining -= alloc;
                        active_weight -= e.weight;
                        new_freeze = true;
                    }
                }
                if !new_freeze {
                    break;
                }
            }
            // Final measure: frozen at their floor, others at the
            // re-divided share. Snapshot the pool view first so the
            // recursive `layout.measure` call below can re-borrow
            // `layout` mutably (and may itself push more entries
            // onto `stack_fill.pool` for nested stacks).
            let pool_end = layout.scratch.stack_fill.pool.len();
            for i in pool_start..pool_end {
                let e = layout.scratch.stack_fill.pool[i];
                let main_avail = match e.frozen_alloc {
                    Some(a) => a,
                    None if active_weight > 0.0 => (remaining * e.weight / active_weight)
                        .min(e.cap)
                        .max(e.floor),
                    None => e.floor,
                };
                let d = layout.measure(
                    tree,
                    e.node,
                    axis.compose_size(main_avail, cross_avail),
                    text,
                    out,
                );
                fill_main += axis.main(d);
                max_cross = max_cross.max(axis.cross(d));
            }
            // Drop our entries — restores the pool length the parent
            // stack saw.
            layout.scratch.stack_fill.pool.truncate(pool_start);
        } else {
            for c in tree.active_children(node) {
                let Sizing::Fill(_) = axis.main_sizing(tree.records.layout()[c.index()].size)
                else {
                    continue;
                };
                let d = layout.measure(
                    tree,
                    c,
                    axis.compose_size(f32::INFINITY, cross_avail),
                    text,
                    out,
                );
                fill_main += axis.main(d);
                max_cross = max_cross.max(axis.cross(d));
            }
        }
    }

    axis.compose_size(sum_non_fill_main + fill_main + total_gap, max_cross)
}

pub(crate) fn arrange(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    inner: Rect,
    axis: Axis,
    out: &mut Layout,
) {
    let panel = tree.panel(node);
    let (gap, justify, parent_child_align) = (panel.gaps.gap(), panel.justify, panel.child_align);

    // Sum desired along main axis for ALL children. Measure has
    // already done the floor-aware Fill distribution; each child's
    // `desired.main` is the slot it should occupy. Arrange just walks
    // them in order. (Older revision recomputed Fill shares here as
    // `leftover * weight / total_weight`, which ignored min-content
    // floors and let Fixed descendants overflow when one Fill sibling
    // had rigid content.)
    let mut sum_main_desired = 0.0f32;
    let mut total_weight = 0.0f32;
    let mut count = 0usize;
    for c in tree.active_children(node) {
        let l = tree.records.layout()[c.index()];
        if let Sizing::Fill(weight) = axis.main_sizing(l.size) {
            total_weight += weight;
        }
        sum_main_desired += axis.main(layout.scratch.desired[c.index()]);
        count += 1;
    }
    let total_gap = gap * count.saturating_sub(1) as f32;

    let main_total = axis.main(inner.size);
    let cross = axis.cross(inner.size);
    let leftover = (main_total - sum_main_desired - total_gap).max(0.0);

    // `justify` distributes any unused main-axis space. With Fill children
    // present, leftover is consumed by Fill weights → justify is a no-op
    // (degrade to Start / original gap).
    let JustifyOffsets {
        start: start_offset,
        gap: effective_gap,
    } = if total_weight > 0.0 {
        JustifyOffsets { start: 0.0, gap }
    } else {
        justify_offsets(justify, leftover, gap, count)
    };

    let cross_min = axis.cross_v(inner.min);
    let mut cursor = axis.main_v(inner.min) + start_offset;
    let mut first = true;

    for child in tree.children(node) {
        let c = child.id;
        if child.visibility.is_collapsed() {
            zero_subtree(layout, tree, c, axis.compose_point(cursor, cross_min), out);
            continue;
        }
        let s = tree.records.layout()[c.index()];
        let d = layout.scratch.desired[c.index()];
        if !first {
            cursor += effective_gap;
        }
        first = false;

        let main_size = axis.main(d);

        let cross_p = cross_place(axis, &s, parent_child_align, d, cross);

        let child_rect =
            axis.compose_rect(cursor, cross_min + cross_p.offset, main_size, cross_p.size);
        layout.arrange(tree, c, child_rect, out);
        cursor += main_size;
    }
}

/// Intrinsic size of a stack on `query_axis` under `req`. When the query
/// axis matches the stack's `main_axis`, sum children's intrinsic on
/// that axis plus gaps; otherwise (cross axis), max over children.
pub(crate) fn intrinsic(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    main_axis: Axis,
    query_axis: Axis,
    req: LenReq,
    text: &TextShaper,
) -> f32 {
    let gap = tree.panel(node).gaps.gap();
    if main_axis == query_axis {
        let mut total = 0.0_f32;
        let mut count = 0_usize;
        for c in tree.active_children(node) {
            total += layout.intrinsic(tree, c, query_axis, req, text);
            count += 1;
        }
        total + gap * count.saturating_sub(1) as f32
    } else {
        children_max_intrinsic(layout, tree, node, query_axis, req, text)
    }
}

#[cfg(test)]
mod tests;
