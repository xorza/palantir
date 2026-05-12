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

use super::axis::Axis;
use super::intrinsic::LenReq;
use super::layoutengine::LayoutEngine;
use super::support::{JustifyOffsets, cross_place, justify_offsets, zero_subtree};
use crate::forest::tree::{NodeId, Tree};
use crate::layout::Layout;
use crate::layout::types::sizing::{Sizes, Sizing};
use crate::primitives::{rect::Rect, size::Size};
use crate::text::TextShaper;

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

/// Flat per-frame scratch for wrap arrange. One contiguous
/// `Vec<NodeId>` pool serves all nesting depths: each `enter()`
/// pushes the current pool length onto `starts`, so the depth's
/// line buffer is `pool[starts.last()..pool.len()]`. `exit()`
/// truncates back to its start. Capacity retained across frames;
/// steady state is alloc-free.
///
/// Why flat (not `Vec<Vec<NodeId>>`): a single allocation for the
/// pool plus a tiny stack of u32 markers, vs. one inner Vec per
/// nesting depth. `place_line` accesses children by index — `NodeId`
/// is `Copy`, so we read each child out before calling
/// `layout.arrange`, sidestepping the borrow conflict that a slice
/// would create against `&mut LayoutEngine`.
#[derive(Default)]
pub(crate) struct WrapScratch {
    pool: Vec<NodeId>,
    /// Stack of per-depth start offsets into `pool`. `enter()` pushes
    /// the current pool length; `exit()` pops and truncates pool
    /// back to that length, releasing this depth's buffer space for
    /// reuse by sibling/parent depths.
    starts: Vec<u32>,
}

impl WrapScratch {
    fn enter(&mut self) {
        self.starts.push(self.pool.len() as u32);
    }

    fn exit(&mut self) {
        let start = self
            .starts
            .pop()
            .expect("WrapScratch::exit called outside enter()");
        self.pool.truncate(start as usize);
    }

    /// Start offset of the current depth's line buffer in `pool`.
    fn start(&self) -> u32 {
        *self
            .starts
            .last()
            .expect("WrapScratch::start called outside enter()")
    }
}

/// Pack children into lines; return content size (max-line-main, sum
/// line-cross + line-gaps). Each call recomputes the packing — cheap
/// (one pass over children), and arrange uses the same logic on the
/// same `desired` values, so the assignment is deterministic across
/// both passes.
pub(crate) fn measure(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    inner: Size,
    axis: Axis,
    text: &TextShaper,
    out: &mut Layout,
) -> Size {
    let panel = tree.panel(node);
    let gap = panel.gap;
    let line_gap = panel.line_gap;
    let main_avail = axis.main(inner);
    let cross_avail = axis.cross(inner);

    // Measure each non-collapsed child once. Pass `INF` on main with the
    // committed cross — same height-given-width pattern as Stack pass-1
    // (so wrap text in a child shapes against `cross_avail`).
    let mut max_line_main = 0.0f32;
    let mut total_cross = 0.0f32;
    let mut line_main = 0.0f32;
    let mut line_cross = 0.0f32;
    let mut line_count = 0usize;

    for c in tree.active_children(node) {
        let d = layout.measure(
            tree,
            c,
            axis.compose_size(f32::INFINITY, cross_avail),
            text,
            out,
        );
        let ChildPack { m, x } = child_pack(axis, tree.records.layout()[c.index()].size, d);
        if would_wrap(line_main, gap, m, main_avail) {
            max_line_main = max_line_main.max(line_main);
            total_cross += line_cross;
            line_count += 1;
            line_main = m;
            line_cross = x;
        } else {
            line_main = if line_main > 0.0 {
                line_main + gap + m
            } else {
                m
            };
            line_cross = line_cross.max(x);
        }
    }
    // Flush last line.
    if line_main > 0.0 {
        max_line_main = max_line_main.max(line_main);
        total_cross += line_cross;
        line_count += 1;
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
    let gap = panel.gap;
    let line_gap = panel.line_gap;
    let justify = panel.justify;
    let parent_child_align = panel.child_align;
    let main_avail = axis.main(inner.size);

    // Same packing logic as `measure`. Each row needs lookahead —
    // can't place a child until we know the row's `line_main` (for
    // justify) and `line_cross` (for cross-axis place). Buffer node
    // IDs in the engine's flat `wrap.pool` at this depth's slice,
    // flush on overflow / end-of-children. Sizes come from
    // `layout.scratch.desired` at flush time, so the buffer is just node
    // IDs.
    layout.scratch.wrap.enter();
    let line_start = layout.scratch.wrap.start();
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
            let d = layout.scratch.desired[c.index()];
            let s = tree.records.layout()[c.index()];
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
            layout.arrange(tree, c, child_rect, out);
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

        let d = layout.scratch.desired[c.index()];
        let ChildPack { m, x } = child_pack(axis, tree.records.layout()[c.index()].size, d);
        if would_wrap(line_main, gap, m, main_avail) {
            place_line(
                layout,
                out,
                line_main,
                line_cross,
                &mut cross_cursor,
                &mut first_line,
            );
            layout.scratch.wrap.pool.push(c);
            line_main = m;
            line_cross = x;
        } else {
            layout.scratch.wrap.pool.push(c);
            line_main = if line_main > 0.0 {
                line_main + gap + m
            } else {
                m
            };
            line_cross = line_cross.max(x);
        }
    }
    place_line(
        layout,
        out,
        line_main,
        line_cross,
        &mut cross_cursor,
        &mut first_line,
    );
    layout.scratch.wrap.exit();
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
    text: &TextShaper,
) -> f32 {
    let gap = tree.panel(node).gap;
    if main_axis == query_axis {
        match req {
            LenReq::MinContent => {
                let mut floor = 0.0f32;
                for c in tree.active_children(node) {
                    floor = floor.max(layout.intrinsic(tree, c, query_axis, req, text));
                }
                floor
            }
            LenReq::MaxContent => {
                let mut total = 0.0f32;
                let mut count = 0usize;
                for c in tree.active_children(node) {
                    total += layout.intrinsic(tree, c, query_axis, req, text);
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
        let mut max = 0.0f32;
        for c in tree.active_children(node) {
            max = max.max(layout.intrinsic(tree, c, query_axis, req, text));
        }
        max
    }
}

#[cfg(test)]
mod tests;
