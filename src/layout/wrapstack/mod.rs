//! WrapStack driver: HStack/VStack with overflow wrap. Children flow on
//! the main axis; when the next child wouldn't fit in the remaining
//! main-axis budget, they wrap to a new line. Cross-axis = sum of line
//! cross-extents + line gaps.
//!
//! Two gap fields: `gap` is within-line sibling spacing (same role as
//! Stack's gap); `line_gap` is between-line spacing.
//!
//! `Sizing::Fill` on the main axis is treated as `Hug` here — wrap
//! semantics conflict with "consume row leftover" and need explicit
//! per-line distribution that's outside this MVP. Cross-axis Fill works
//! identically to Stack: each line's cross size = max child cross, and
//! `place_axis` with the `Auto-stretches-Fill` rule makes Fill children
//! grow to that height (CSS `align-items: stretch` default).

use crate::forest::tree::Tree;
use crate::forest::tree::node::NodeId;
use crate::layout::Layout;
use crate::layout::axis::Axis;
use crate::layout::engine::LayoutEngine;
use crate::layout::intrinsic::LenReq;
use crate::layout::support::{
    JustifyOffsets, TextCtx, children_max_intrinsic, cross_place, justify_offsets, zero_subtree,
};
use crate::layout::types::sizing::{Sizes, Sizing};
use crate::primitives::{rect::Rect, size::Size};

/// One child's contribution to the current line. `m` always comes from
/// the child's main-axis desired size; `x` is the cross contribution
/// the line should hug to, zero for Fill-on-cross children (CSS flex
/// parity — Fill stretches to the row, doesn't drive its height).
struct ChildPack {
    m: f32,
    x: f32,
}

#[inline]
fn child_pack(axis: Axis, child_size: Sizes, d: Size) -> ChildPack {
    ChildPack {
        m: axis.main(d),
        x: if matches!(axis.cross_sizing(child_size), Sizing::Fill(_)) {
            0.0
        } else {
            axis.cross(d)
        },
    }
}

/// True iff appending a child of main-axis extent `m` to the current
/// line would push it past `main_avail`. The first child on a line
/// (`line_main == 0`) never wraps — it always sets the line's start.
#[inline]
fn would_wrap(line_main: f32, gap: f32, m: f32, main_avail: f32) -> bool {
    line_main > 0.0 && line_main + gap + m > main_avail
}

/// Advance the line-packing state by one child. When the child won't fit
/// on the current line, `complete_line(line_main, line_cross)` runs for
/// the just-finished line and a fresh line starts with this child;
/// otherwise the current line extends. The wrap decision **and** the
/// line-extent arithmetic live here so measure and arrange can't drift on
/// where lines break.
#[inline]
fn pack_child(
    line_main: &mut f32,
    line_cross: &mut f32,
    gap: f32,
    main_avail: f32,
    pack: ChildPack,
    mut complete_line: impl FnMut(f32, f32),
) {
    let ChildPack { m, x } = pack;
    if would_wrap(*line_main, gap, m, main_avail) {
        complete_line(*line_main, *line_cross);
        *line_main = m;
        *line_cross = x;
    } else {
        *line_main = if *line_main > 0.0 {
            *line_main + gap + m
        } else {
            m
        };
        *line_cross = line_cross.max(x);
    }
}

/// Flat per-frame scratch for wrap arrange. One contiguous
/// `Vec<NodeId>` pool serves all nesting depths: each `arrange`
/// captures the pool length on entry as its depth's start, and
/// `place_line` truncates back to that start after every flushed line
/// — so the pool is empty-at-this-depth again before any recursion or
/// the next line's pushes. Capacity retained across frames; steady
/// state is alloc-free.
///
/// Why flat (not `Vec<Vec<NodeId>>`): a single allocation for the
/// pool vs. one inner Vec per nesting depth. `place_line` accesses
/// children by index — `NodeId` is `Copy`, so we read each child out
/// before calling `layout.arrange`, sidestepping the borrow conflict
/// that a slice would create against `&mut LayoutEngine`.
#[derive(Default)]
pub(crate) struct WrapScratch {
    pool: Vec<NodeId>,
}

/// Pack children into lines; return content size (max-line-main, sum
/// line-cross + line-gaps). Each call recomputes the packing — cheap
/// (one pass over children), and arrange uses the same logic on the
/// same `desired` values, so the assignment is deterministic across
/// both passes.
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
    let panel = tree.panel(node);
    let gap = panel.gaps.gap();
    let line_gap = panel.gaps.line_gap();
    let main_avail = axis.main(inner_avail);
    let cross_avail = axis.cross(inner_avail);

    // Measure each non-collapsed child once. Pass `INF` on main with the
    // committed cross — same height-given-width pattern as Stack pass-1
    // (so wrap text in a child shapes against `cross_avail`).
    let layouts = tree.records.layout();
    let mut max_line_main = 0.0f32;
    let mut total_cross = 0.0f32;
    let mut line_main = 0.0f32;
    let mut line_cross = 0.0f32;
    let mut line_count = 0usize;

    let mut complete_line = |lm: f32, lx: f32| {
        max_line_main = max_line_main.max(lm);
        total_cross += lx;
        line_count += 1;
    };
    for c in tree.active_children(node) {
        let d = layout.measure(
            tree,
            c,
            axis.compose_size(f32::INFINITY, cross_avail),
            tc,
            out,
        );
        let pack = child_pack(axis, layouts[c.idx()].size, d);
        pack_child(
            &mut line_main,
            &mut line_cross,
            gap,
            main_avail,
            pack,
            &mut complete_line,
        );
    }
    // Flush last line.
    if line_main > 0.0 {
        complete_line(line_main, line_cross);
    }
    // `n_lines - 1` line gaps between lines.
    if line_count > 1 {
        total_cross += line_gap * (line_count - 1) as f32;
    }

    axis.compose_size(max_line_main, total_cross)
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
    let gap = panel.gaps.gap();
    let line_gap = panel.gaps.line_gap();
    let justify = panel.justify;
    let parent_child_align = panel.child_align;
    let main_avail = axis.main(inner.size);
    let self_outer = out[layout.active_layer].rect[node.idx()].size;

    // Same packing logic as `measure`. Each row needs lookahead —
    // can't place a child until we know the row's `line_main` (for
    // justify) and `line_cross` (for cross-axis place). Buffer node
    // IDs in the engine's flat `wrap.pool` at this depth's slice,
    // flush on overflow / end-of-children. Sizes come from
    // `layout.scratch.desired` at flush time, so the buffer is just node
    // IDs.
    let layouts = tree.records.layout();
    let line_start = layout.scratch.wrap.pool.len() as u32;
    let mut line_main = 0.0f32;
    let mut line_cross = 0.0f32;
    let mut cross_cursor = axis.cross_v(inner.min);
    let mut first_line = true;

    let place_line = |layout: &mut LayoutEngine,
                      out: &mut Layout,
                      line_main: f32,
                      line_cross: f32,
                      cross_cursor: &mut f32,
                      first_line: &mut bool| {
        let line_end = layout.scratch.wrap.pool.len();
        let line_start = line_start as usize;
        if line_end == line_start {
            return;
        }
        if !*first_line {
            *cross_cursor += line_gap;
        }
        *first_line = false;

        let count = line_end - line_start;
        let leftover = (main_avail - line_main).max(0.0);
        let JustifyOffsets {
            start: start_offset,
            gap: eff_gap,
        } = justify_offsets(justify, leftover, gap, count);
        let mut main_cursor = axis.main_v(inner.min) + start_offset;
        // Iterate by index so we copy each `NodeId` out before
        // calling `layout.arrange`, which needs `&mut layout`.
        // `NodeId` is `Copy`, so no slice borrow into the pool.
        for i in line_start..line_end {
            let c = layout.scratch.wrap.pool[i];
            if i > line_start {
                main_cursor += eff_gap;
            }
            let i = c.idx();
            let d = layout.scratch.desired[i];
            let s = layouts[i];
            // Cross axis: each child placed within the line's cross
            // extent. Same rule as Stack cross — Fill stretches to
            // line_cross, Hug aligns per child.
            let cross_p = cross_place(axis, &s, parent_child_align, d, line_cross);
            let main_size = axis.main(d);
            let child_rect = axis.compose_rect(
                main_cursor,
                *cross_cursor + cross_p.offset,
                main_size,
                cross_p.size,
            );
            layout.arrange(tree, c, self_outer, child_rect, out);
            main_cursor += main_size;
        }
        *cross_cursor += line_cross;
        // Drop our line from the pool (capacity retained). Recursive
        // `layout.arrange` calls above may have temporarily extended
        // and re-truncated the pool past `line_end`; we ignore those
        // and reset to our depth's start.
        layout.scratch.wrap.pool.truncate(line_start);
    };

    // Walk all children: collapsed get zeroed at the cursor, active
    // children pack into the current line and flush on overflow.
    for child in tree.children(node) {
        let c = child.id;
        if child.visibility.is_collapsed() {
            // Anchor inside this layout's inner rect at the current
            // cursor. Position is stable; size is zero so there's no
            // visual or input contribution.
            zero_subtree(
                layout,
                tree,
                c,
                axis.compose_point(axis.main_v(inner.min), cross_cursor),
                out,
            );
            continue;
        }

        let i = c.idx();
        let d = layout.scratch.desired[i];
        let pack = child_pack(axis, layouts[i].size, d);
        // On wrap, `pack_child` places the just-finished line (which
        // empties the pool back to this depth's start); the child that
        // triggered the wrap is then pushed as the new line's first node.
        pack_child(
            &mut line_main,
            &mut line_cross,
            gap,
            main_avail,
            pack,
            |lm, lx| place_line(layout, out, lm, lx, &mut cross_cursor, &mut first_line),
        );
        layout.scratch.wrap.pool.push(c);
    }
    place_line(
        layout,
        out,
        line_main,
        line_cross,
        &mut cross_cursor,
        &mut first_line,
    );
}

/// Intrinsic size on `query_axis` under `req`. Approximate for the wrap
/// case — we don't run the full packing here, so cross-axis answers
/// assume single-line layout (the conservative max-content shape). Main
/// axis answers are exact:
///
/// - **MinContent** on main: max child intrinsic on main (the widest
///   single child sets the floor; smaller-than-that and even one row
///   overflows).
/// - **MaxContent** on main: sum + within-line gaps (single line).
/// - Cross axis: max child intrinsic (single-line approximation).
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
        match req {
            // Widest single child sets the floor — one row of just that
            // child still has to fit.
            LenReq::MinContent => children_max_intrinsic(layout, tree, node, query_axis, req, tc),
            LenReq::MaxContent => {
                let mut total = 0.0f32;
                let mut count = 0usize;
                for c in tree.active_children(node) {
                    total += layout.intrinsic(tree, c, query_axis, req, tc);
                    count += 1;
                }
                total + gap * count.saturating_sub(1) as f32
            }
        }
    } else {
        // Cross-axis approximation: max child intrinsic on cross. Real
        // wrapped cross depends on resolved main width — height-given-
        // width — which we don't compute here. Conservative for typical
        // toolbar/badge use cases.
        children_max_intrinsic(layout, tree, node, query_axis, req, tc)
    }
}

#[cfg(test)]
mod tests;
