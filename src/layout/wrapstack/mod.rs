//! WrapStack driver: HStack/VStack with overflow wrap. Children flow on
//! the main axis; when the next child wouldn't fit in the remaining
//! main-axis budget, they wrap to a new line. Cross-axis = sum of line
//! cross-extents + line gaps.
//!
//! Two gap fields: `gap` is within-line sibling spacing (same role as
//! Stack's gap); `line_gap` is between-line spacing.
//!
//! `Sizing::fill` on the main axis is treated as `Hug` here — wrap
//! semantics conflict with "consume row leftover" and need explicit
//! per-line distribution that's outside this MVP. Cross-axis Fill works
//! identically to Stack: each line's cross size = max child cross, and
//! shared arrange-axis resolution makes Fill children grow to that
//! height without shrinking below their measured content.

use crate::layout::axis::Axis;
use crate::layout::engine::LayoutEngine;
use crate::layout::intrinsic::{IntrinsicQuery, IntrinsicRange, LenReq};
use crate::layout::pass::LayoutPass;
use crate::layout::support::{
    JustifyOffsets, children_max_intrinsic, cross_place, justify_offsets, no_offset,
};
use crate::primitives::interned_str::InternedText;
use crate::primitives::{rect::Rect, size::Size};
use crate::scene::tree::Tree;
use crate::scene::tree::record::NodeId;

/// One child's measured contribution to the current line.
#[derive(Clone, Copy, Debug)]
struct ChildPack {
    main: f32,
    cross: f32,
}

#[derive(Clone, Copy, Debug, Default)]
struct LinePack {
    main: f32,
    cross: f32,
    occupied: bool,
}

#[inline]
fn child_pack(axis: Axis, d: Size) -> ChildPack {
    ChildPack {
        main: axis.main(d),
        cross: axis.cross(d),
    }
}

/// True iff appending a child to the current line would push it past
/// `main_avail`. The first child on an empty line never wraps.
#[inline]
fn would_wrap(line: LinePack, gap: f32, child_main: f32, main_avail: f32) -> bool {
    line.occupied && line.main + gap + child_main > main_avail
}

/// Advance the line-packing state by one child. When the child won't fit
/// on the current line, `complete_line(line_main, line_cross)` runs for
/// the just-finished line and a fresh line starts with this child;
/// otherwise the current line extends. The wrap decision **and** the
/// line-extent arithmetic live here so measure and arrange can't drift on
/// where lines break.
#[inline]
fn pack_child(
    line: &mut LinePack,
    gap: f32,
    main_avail: f32,
    pack: ChildPack,
    mut complete_line: impl FnMut(f32, f32),
) {
    let ChildPack { main, cross } = pack;
    if would_wrap(*line, gap, main, main_avail) {
        complete_line(line.main, line.cross);
        *line = LinePack {
            main,
            cross,
            occupied: true,
        };
    } else {
        if line.occupied {
            line.main += gap;
        }
        line.main += main;
        line.cross = line.cross.max(cross);
        line.occupied = true;
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
#[derive(Debug, Default)]
pub(crate) struct WrapScratch {
    pool: Vec<NodeId>,
}

impl WrapScratch {
    #[inline]
    fn len(&self) -> usize {
        self.pool.len()
    }

    #[inline]
    fn push(&mut self, node: NodeId) {
        self.pool.push(node);
    }

    #[inline]
    fn at(&self, index: usize) -> NodeId {
        self.pool[index]
    }

    #[inline]
    fn truncate(&mut self, len: usize) {
        self.pool.truncate(len);
    }
}

/// Pack children into lines; return content size (max-line-main, sum
/// line-cross + line-gaps). Each call recomputes the packing — cheap
/// (one pass over children), and arrange uses the same logic on the
/// same `desired` values, so the assignment is deterministic across
/// both passes.
#[profiling::function]
pub(super) fn measure(
    pass: &mut LayoutPass<'_>,
    node: NodeId,
    inner_avail: Size,
    axis: Axis,
) -> Size {
    let tree = pass.tree;
    let panel = tree.panel(node);
    let gap = panel.gaps.gap();
    let line_gap = panel.gaps.line_gap();
    let main_avail = axis.main(inner_avail);
    let cross_avail = axis.cross(inner_avail);

    // Measure each non-collapsed child once. Pass `INF` on main with the
    // committed cross — same height-given-width pattern as Stack pass-1
    // (so wrap text in a child shapes against `cross_avail`).
    let mut max_line_main = 0.0f32;
    let mut total_cross = 0.0f32;
    let mut line = LinePack::default();
    let mut line_count = 0usize;

    let mut complete_line = |line_main: f32, line_cross: f32| {
        max_line_main = max_line_main.max(line_main);
        total_cross += line_cross;
        line_count += 1;
    };
    for c in tree.active_children(node) {
        let d = pass.measure(c, axis.compose_size(f32::INFINITY, cross_avail));
        let pack = child_pack(axis, d);
        pack_child(&mut line, gap, main_avail, pack, &mut complete_line);
    }
    // Flush last line.
    if line.occupied {
        complete_line(line.main, line.cross);
    }
    // `n_lines - 1` line gaps between lines.
    if line_count > 1 {
        total_cross += line_gap * (line_count - 1) as f32;
    }

    axis.compose_size(max_line_main, total_cross)
}

pub(super) fn arrange(pass: &mut LayoutPass<'_>, node: NodeId, inner: Rect, axis: Axis) {
    let tree = pass.tree;
    let panel = tree.panel(node);
    let gap = panel.gaps.gap();
    let line_gap = panel.gaps.line_gap();
    let justify = panel.justify;
    let parent_child_align = panel.child_align;
    let main_avail = axis.main(inner.size);

    // Same packing logic as `measure`. Each row needs lookahead —
    // can't place a child until we know the row's `line_main` (for
    // justify) and `line_cross` (for cross-axis place). Buffer node
    // IDs in the engine's flat `wrap.pool` at this depth's slice,
    // flush on overflow / end-of-children. Sizes come from
    // `pass.desired(..)` at flush time, so the buffer is just node
    // IDs.
    let layouts = tree.records.layout();
    let line_start = pass.wrap_scratch_mut().len() as u32;
    let mut line = LinePack::default();
    let mut cross_cursor = axis.cross_v(inner.min);
    let mut first_line = true;

    let place_line = |pass: &mut LayoutPass<'_>,
                      line_main: f32,
                      line_cross: f32,
                      cross_cursor: &mut f32,
                      first_line: &mut bool| {
        let line_end = pass.wrap_scratch_mut().len();
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
            let c = pass.wrap_scratch_mut().at(i);
            if i > line_start {
                main_cursor += eff_gap;
            }
            let d = pass.desired(c);
            let s = layouts[c.idx()];
            // Cross axis: each child placed within the line's cross
            // extent. Same rule as Stack cross — Fill stretches to
            // line_cross, Hug aligns per child.
            let bounds = tree.bounds(c);
            let cross_p = cross_place(axis, &s, bounds, parent_child_align, d, line_cross);
            let main_size = axis.main(d);
            let child_rect = axis.compose_rect(
                main_cursor,
                *cross_cursor + cross_p.offset,
                main_size,
                cross_p.size,
            );
            pass.arrange(c, child_rect);
            main_cursor += main_size;
        }
        *cross_cursor += line_cross;
        // Drop our line from the pool (capacity retained). Recursive
        // `layout.arrange` calls above may have temporarily extended
        // and re-truncated the pool past `line_end`; we ignore those
        // and reset to our depth's start.
        pass.wrap_scratch_mut().truncate(line_start);
    };

    // Walk all children: collapsed get zeroed at the cursor, active
    // children pack into the current line and flush on overflow.
    for child in tree.children(node) {
        let c = child.id;
        if child.visibility.is_collapsed() {
            // Anchor inside this layout's inner rect at the current
            // cursor. Position is stable; size is zero so there's no
            // visual or input contribution.
            pass.zero_subtree(c, axis.compose_point(axis.main_v(inner.min), cross_cursor));
            continue;
        }

        let d = pass.desired(c);
        let pack = child_pack(axis, d);
        // On wrap, `pack_child` places the just-finished line (which
        // empties the pool back to this depth's start); the child that
        // triggered the wrap is then pushed as the new line's first node.
        pack_child(&mut line, gap, main_avail, pack, |line_main, line_cross| {
            place_line(
                pass,
                line_main,
                line_cross,
                &mut cross_cursor,
                &mut first_line,
            )
        });
        pass.wrap_scratch_mut().push(c);
    }
    if line.occupied {
        place_line(
            pass,
            line.main,
            line.cross,
            &mut cross_cursor,
            &mut first_line,
        );
    }
}

/// Intrinsic size on `query_axis`. Approximate for the wrap
/// case — we don't run the full packing here, so cross-axis answers
/// assume single-line layout (the conservative max-content shape). Main
/// axis answers are exact:
///
/// - **MinContent** on main: max child intrinsic on main (the widest
///   single child sets the floor; smaller-than-that and even one row
///   overflows).
/// - **MaxContent** on main: sum + within-line gaps (single line).
/// - Cross axis: max child intrinsic (single-line approximation).
pub(super) fn intrinsic(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    main_axis: Axis,
    query_axis: Axis,
    query: IntrinsicQuery,
    interned_text: &InternedText<'_>,
) -> IntrinsicRange {
    if main_axis != query_axis {
        // Cross-axis approximation: max child intrinsic on cross. Real
        // wrapped cross depends on resolved main width — height-given-
        // width — which we don't compute here. Conservative for typical
        // toolbar/badge use cases.
        return children_max_intrinsic(
            layout,
            tree,
            node,
            query_axis,
            query,
            interned_text,
            no_offset,
        );
    }
    let mut range = IntrinsicRange::ZERO;
    let mut count = 0_usize;
    for c in tree.active_children(node) {
        let child = query.child(layout, tree, c, query_axis, interned_text);
        for (req, slot) in range.requested(query) {
            *slot = match req {
                // The widest single child is the floor: narrower than
                // that and even a one-child line overflows.
                LenReq::MinContent => slot.max(child.min),
                // Everything on one line.
                LenReq::MaxContent => *slot + child.max,
            };
        }
        count += 1;
    }
    // Within-line gaps exist only on the single-line (max) reading.
    if query.includes(LenReq::MaxContent) {
        range.max += tree.panel(node).gaps.gap() * count.saturating_sub(1) as f32;
    }
    range
}

#[cfg(test)]
mod tests;
