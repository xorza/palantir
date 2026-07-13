use crate::forest::tree::{NodeId, Tree};
use crate::layout::Layout;
use crate::layout::axis::Axis;
use crate::layout::engine::LayoutEngine;
use crate::layout::intrinsic::LenReq;
use crate::layout::support::{
    JustifyOffsets, TextCtx, children_max_intrinsic, cross_place, justify_offsets, zero_subtree,
};
use crate::layout::types::sizing::Sizing;
use crate::primitives::{rect::Rect, size::Size};

/// One Fill child as the freeze loop sees it. Pushed onto
/// `LayoutScratch::stack_fill` during measure; popped at the end of
/// the call. `frozen_alloc = Some(v)` means this child has been
/// removed from the active pool and gets exactly `v` main-axis space.
#[derive(Clone, Copy)]
pub(crate) struct FillEntry {
    node: NodeId,
    weight: f32,
    /// Minimum main-axis extent this entry will accept. The freeze
    /// loop pins an entry at `floor` when its weighted share would be
    /// lower. Source depends on phase: `measure` uses the child's
    /// `intrinsic(MinContent)` (largest non-shrinkable descendant);
    /// `arrange` uses the child's measured `desired.main` (post-WPF
    /// Stretch this is the child's content size). Invariant:
    /// arrange-floor ≥ measure-floor for the same child, since
    /// `resolve_axis_size` floors `desired` by `intrinsic_min`.
    floor: f32,
    cap: f32,
    frozen_alloc: Option<f32>,
}

/// Distribute `leftover` across the Fill entries by weight, with
/// CSS-Flexbox-style freezing: any child whose weighted share falls
/// outside its `[floor, cap]` takes the violated bound and exits the
/// pool; the remaining children re-share. After the loop, every entry's
/// `frozen_alloc` is `Some(_)` — either set during a freeze pass or
/// filled in at the end with the final share. Shared by `measure` (floor
/// = `intrinsic(MinContent)`, leftover from the parent's `inner.main`)
/// and `arrange` (floor = `desired.main`, leftover from the parent's
/// arranged slot). Same algorithm in both phases — the only difference
/// is the floor source the caller pushes into each entry.
///
/// Grid's Phase-3 Fill loop (`grid::resolve_axis`) solves the identical
/// `[lo, hi]`-clamped weighted distribution. The two are kept in sync by
/// hand rather than physically merged: this one freezes every violator
/// per pass while grid freezes one, and the two converge differently for
/// mixed min/max violations — so a shared solver would silently change
/// one driver's edge-case results.
fn freeze_distribute(entries: &mut [FillEntry], mut leftover: f32, mut active_weight: f32) {
    loop {
        if active_weight <= 0.0 {
            break;
        }
        let mut new_freeze = false;
        for e in entries.iter_mut() {
            if e.frozen_alloc.is_some() {
                continue;
            }
            let share = leftover * e.weight / active_weight;
            // Freeze any entry whose proportional share falls outside
            // `[floor, cap]`: it takes the violated bound and the rest
            // re-divide (CSS Flexbox-style). A `cap` below `floor`
            // resolves to `cap` — the hard max wins.
            if e.floor > share || share > e.cap {
                let alloc = if e.floor > share {
                    e.floor.min(e.cap)
                } else {
                    e.cap
                };
                e.frozen_alloc = Some(alloc);
                leftover -= alloc;
                active_weight -= e.weight;
                new_freeze = true;
            }
        }
        if !new_freeze {
            break;
        }
    }
    for e in entries.iter_mut() {
        if e.frozen_alloc.is_none() {
            let share = if active_weight > 0.0 {
                leftover * e.weight / active_weight
            } else {
                e.floor
            };
            // In-pool entries satisfy `floor <= share <= cap`; the
            // `active_weight == 0` fallback (`share = floor`) still clamps
            // so a `cap < floor` entry never exceeds its hard max.
            e.frozen_alloc = Some(share.clamp(e.floor.min(e.cap), e.cap));
        }
    }
}

/// Flat depth-shared buffer for the Fill freeze loop. Layout is the
/// same as `WrapScratch.pool`: each invocation pushes its entries,
/// uses the resulting slice, truncates on exit so nested stacks
/// reuse the tail capacity. Allocation-free in steady state.
#[derive(Default)]
pub(crate) struct StackScratch {
    pub(crate) pool: Vec<FillEntry>,
}

/// Push one [`FillEntry`] per main-axis-`Fill` child onto
/// `layout.scratch.stack_fill.pool`. Returns the pool index where this
/// invocation's slice starts — pair with `pool.truncate(start)` after
/// use to keep nested calls reentrant. `floor_for` produces each
/// child's min-content floor: `measure` passes
/// `intrinsic(MinContent)`, `arrange` passes the measured
/// `desired.main`. Shared so the two phases can't drift on which
/// children get pushed, what their cap is, or how the pool is keyed.
fn push_fill_entries(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    axis: Axis,
    mut floor_for: impl FnMut(&mut LayoutEngine, NodeId) -> f32,
) -> usize {
    let layouts = tree.records.layout();
    let pool_start = layout.scratch.stack_fill.pool.len();
    for c in tree.active_children(node) {
        let Sizing::Fill(w) = axis.main_sizing(layouts[c.idx()].size) else {
            continue;
        };
        let cap = axis.main(tree.bounds(c).max_size);
        let floor = floor_for(layout, c);
        layout.scratch.stack_fill.pool.push(FillEntry {
            node: c,
            weight: w,
            floor,
            cap,
            frozen_alloc: None,
        });
    }
    pool_start
}

/// Per-phase main-axis accounting for a stack, shared by `measure` and
/// `arrange`. `count` and `total_weight` are pure tree functions; the
/// non-Fill main sum is sourced per phase via `main_of` (measure: fresh
/// `layout.measure`; arrange: cached `desired.main`). Keeps the two
/// phases from drifting on the count / weight / gap construction.
struct StackPlan {
    sum_non_fill_main: f32,
    total_weight: f32,
    count: usize,
    total_gap: f32,
}

fn stack_plan(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    axis: Axis,
    gap: f32,
    mut main_of: impl FnMut(&mut LayoutEngine, NodeId) -> f32,
) -> StackPlan {
    let layouts = tree.records.layout();
    let mut sum_non_fill_main = 0.0f32;
    let mut total_weight = 0.0f32;
    let mut count = 0usize;
    for c in tree.active_children(node) {
        count += 1;
        if let Sizing::Fill(w) = axis.main_sizing(layouts[c.idx()].size) {
            total_weight += w;
        } else {
            sum_non_fill_main += main_of(layout, c);
        }
    }
    StackPlan {
        sum_non_fill_main,
        total_weight,
        count,
        total_gap: gap * count.saturating_sub(1) as f32,
    }
}

#[profiling::function]
pub(crate) fn measure(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    inner_avail: Size,
    axis: Axis,
    tc: &TextCtx<'_>,
    out: &mut Layout,
) -> Size {
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
    let layouts = tree.records.layout();
    // Pass 1 measures non-Fill children (height-given-width). `stack_plan`
    // shares the count / weight / gap accounting with `arrange`; the
    // closure supplies the per-phase main source (here: fresh measurement)
    // and folds in the cross-axis max.
    let mut max_cross = 0.0f32;
    let StackPlan {
        sum_non_fill_main,
        total_weight,
        total_gap,
        ..
    } = stack_plan(layout, tree, node, axis, gap, |layout, c| {
        let d = layout.measure(tree, c, axis.compose_size(main_avail, cross_avail), tc, out);
        max_cross = max_cross.max(axis.cross(d));
        axis.main(d)
    });

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
    // Soundness: the `axis.main(inner_avail)` we use as the budget here
    // must equal the `axis.main(inner.size)` the matching `arrange` call
    // sees, otherwise wrap text in Fill children shapes against the wrong
    // width. It does, because the Stack's outer main size is a
    // deterministic function of (its own `Sizing` + parent-supplied
    // `available`) via `resolve_axis_size`, and the parent passes the
    // same `available` to `measure` that it later derives `slot.placement.size`
    // from for `arrange`. Any future driver that clamps a child's slot
    // *between* its own measure and arrange would break this.
    let mut fill_main = 0.0f32;
    if total_weight > 0.0 {
        let main_finite = axis.main(inner_avail).is_finite();
        if main_finite {
            // Reentrancy-safe flat pool: record pool-end on entry,
            // push entries, slice through `[start..]`, truncate at
            // exit. Nested stacks reuse the tail capacity.
            let pool_start = push_fill_entries(layout, tree, node, axis, |layout, c| {
                layout.intrinsic(tree, c, axis, LenReq::MinContent, tc)
            });
            let leftover = (axis.main(inner_avail) - sum_non_fill_main - total_gap).max(0.0);
            freeze_distribute(
                &mut layout.scratch.stack_fill.pool[pool_start..],
                leftover,
                total_weight,
            );
            // Final measure: each Fill child sees its frozen share
            // as `main_avail`. Snapshot the pool view first so the
            // recursive `layout.measure` call below can re-borrow
            // `layout` mutably (and may itself push more entries
            // onto `stack_fill.pool` for nested stacks).
            let pool_end = layout.scratch.stack_fill.pool.len();
            for i in pool_start..pool_end {
                let e = layout.scratch.stack_fill.pool[i];
                // unwrap: `freeze_distribute` always fills in
                // `frozen_alloc` for every entry.
                let main_avail = e.frozen_alloc.unwrap();
                let d = layout.measure(
                    tree,
                    e.node,
                    axis.compose_size(main_avail, cross_avail),
                    tc,
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
                let Sizing::Fill(_) = axis.main_sizing(layouts[c.idx()].size) else {
                    continue;
                };
                let d = layout.measure(
                    tree,
                    c,
                    axis.compose_size(f32::INFINITY, cross_avail),
                    tc,
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
    let self_outer = out[layout.active_layer].rect[node.idx()].size;

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
    // resolve_axis_size change pins Fill at content).
    let StackPlan {
        sum_non_fill_main,
        total_weight,
        count,
        total_gap,
    } = stack_plan(layout, tree, node, axis, gap, |layout, c| {
        axis.main(layout.scratch.desired[c.idx()])
    });
    // Cap = `max_size`. The freeze loop below mirrors `measure`'s
    // flexbox-style distribution: a child whose share is outside
    // `[floor, cap]` freezes at the bound, the rest re-share remaining.
    let pool_start = push_fill_entries(layout, tree, node, axis, |layout, c| {
        axis.main(layout.scratch.desired[c.idx()])
    });
    let main_total = axis.main(inner.size);
    let cross = axis.cross(inner.size);
    let leftover_for_fill = (main_total - sum_non_fill_main - total_gap).max(0.0);
    freeze_distribute(
        &mut layout.scratch.stack_fill.pool[pool_start..],
        leftover_for_fill,
        total_weight,
    );
    // The sum we report to `justify` is the post-redistribute total —
    // i.e., what the children will *actually* occupy after arrange.
    // unwrap: `freeze_distribute` post-condition guarantees every
    // entry's `frozen_alloc` is `Some(_)`.
    let sum_main_arranged = sum_non_fill_main
        + layout.scratch.stack_fill.pool[pool_start..]
            .iter()
            .map(|e| e.frozen_alloc.unwrap())
            .sum::<f32>();
    let leftover_for_justify = (main_total - sum_main_arranged - total_gap).max(0.0);

    // `justify` distributes any *remaining* main-axis slack. With Fill
    // children that hit their cap (or with zero leftover) we may still
    // have free pixels — justify them out.
    let JustifyOffsets {
        start: start_offset,
        gap: effective_gap,
    } = justify_offsets(justify, leftover_for_justify, gap, count);

    let cross_min = axis.cross_v(inner.min);
    let mut cursor = axis.main_v(inner.min) + start_offset;
    let mut first = true;
    let mut fill_cursor = pool_start;

    for child in tree.children(node) {
        let c = child.id;
        if child.visibility.is_collapsed() {
            zero_subtree(layout, tree, c, axis.compose_point(cursor, cross_min), out);
            continue;
        }
        let i = c.idx();
        let s = layouts[i];
        let d = layout.scratch.desired[i];
        if !first {
            cursor += effective_gap;
        }
        first = false;

        let main_size = if matches!(axis.main_sizing(s.size), Sizing::Fill(_)) {
            // unwrap: every Fill child pushed an entry above and the
            // resolve pass filled in `frozen_alloc`.
            let alloc = layout.scratch.stack_fill.pool[fill_cursor]
                .frozen_alloc
                .unwrap();
            fill_cursor += 1;
            alloc
        } else {
            axis.main(d)
        };

        let cross_p = cross_place(axis, &s, parent_child_align, d, cross);

        let child_rect =
            axis.compose_rect(cursor, cross_min + cross_p.offset, main_size, cross_p.size);
        layout.arrange(tree, c, self_outer, child_rect, out);
        cursor += main_size;
    }
    layout.scratch.stack_fill.pool.truncate(pool_start);
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
    tc: &TextCtx<'_>,
) -> f32 {
    let gap = tree.panel(node).gaps.gap();
    if main_axis == query_axis {
        let mut total = 0.0_f32;
        let mut count = 0_usize;
        for c in tree.active_children(node) {
            total += layout.intrinsic(tree, c, query_axis, req, tc);
            count += 1;
        }
        total + gap * count.saturating_sub(1) as f32
    } else {
        children_max_intrinsic(layout, tree, node, query_axis, req, tc)
    }
}

#[cfg(test)]
mod tests;
