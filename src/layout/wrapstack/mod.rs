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
//! `place_axis` with `AutoBias::StretchIfFill` makes Fill children grow
//! to that height (CSS `align-items: stretch` default).

use super::support::{AutoBias, place_axis, resolved_axis_align, zero_subtree};
use super::{Axis, LayoutEngine, LenReq};
use crate::primitives::{Justify, Rect, Size, Sizing};
use crate::text::TextMeasurer;
use crate::tree::{NodeId, Tree};

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
pub(super) fn measure(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    inner: Size,
    axis: Axis,
    text: &mut TextMeasurer,
) -> Size {
    let extras = tree.read_extras(node);
    let gap = extras.gap;
    let line_gap = extras.line_gap;
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

    for c in tree.children_active(node) {
        let d = layout.measure(tree, c, axis.compose_size(f32::INFINITY, cross_avail), text);
        let m = axis.main(d);
        // CSS flex parity: `Fill` on cross stretches to the row's
        // cross extent rather than driving it. So the line's cross
        // height comes from non-Fill children only — a Fill-on-cross
        // child measured at `cross_avail` would otherwise inflate
        // `line_cross` to the parent's full cross.
        let x = if matches!(axis.cross_sizing(tree.layout(c).size), Sizing::Fill(_)) {
            0.0
        } else {
            axis.cross(d)
        };

        // Try to extend the current line. The first child on a line
        // doesn't pay the within-line gap.
        let candidate = if line_main > 0.0 {
            line_main + gap + m
        } else {
            m
        };

        if line_main > 0.0 && candidate > main_avail {
            // Wrap: flush current line, start a new one with this child.
            max_line_main = max_line_main.max(line_main);
            total_cross += line_cross;
            line_count += 1;
            line_main = m;
            line_cross = x;
        } else {
            line_main = candidate;
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

pub(super) fn arrange(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    inner: Rect,
    axis: Axis,
) {
    let extras = tree.read_extras(node);
    let gap = extras.gap;
    let line_gap = extras.line_gap;
    let justify = extras.justify;
    let parent_child_align = extras.child_align;
    let main_avail = axis.main(inner.size);

    // Same packing logic as `measure`. Each row needs lookahead —
    // can't place a child until we know the row's `line_main` (for
    // justify) and `line_cross` (for cross-axis place). Buffer node
    // IDs in the engine's flat `wrap.pool` at this depth's slice,
    // flush on overflow / end-of-children. Sizes come from
    // `layout.desired` at flush time, so the buffer is just node
    // IDs.
    layout.wrap.enter();
    let line_start = layout.wrap.start();
    let mut line_main = 0.0f32;
    let mut line_cross = 0.0f32;
    let mut cross_cursor = axis.cross_v(inner.min);
    let mut first_line = true;

    let place_line = |layout: &mut LayoutEngine,
                      line_main: f32,
                      line_cross: f32,
                      cross_cursor: &mut f32,
                      first_line: &mut bool| {
        let line_end = layout.wrap.pool.len();
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
        let (start_offset, eff_gap) = match justify {
            Justify::Start => (0.0, gap),
            Justify::Center => (leftover * 0.5, gap),
            Justify::End => (leftover, gap),
            Justify::SpaceBetween if count > 1 => (0.0, gap + leftover / (count - 1) as f32),
            Justify::SpaceAround if count > 0 => {
                let extra = leftover / count as f32;
                (extra * 0.5, gap + extra)
            }
            // Fewer than 2 / 1 children → fallback to Start.
            Justify::SpaceBetween | Justify::SpaceAround => (0.0, gap),
        };
        let mut main_cursor = axis.main_v(inner.min) + start_offset;
        // Iterate by index so we copy each `NodeId` out before
        // calling `layout.arrange`, which needs `&mut layout`.
        // `NodeId` is `Copy`, so no slice borrow into the pool.
        for i in line_start..line_end {
            let c = layout.wrap.pool[i];
            if i > line_start {
                main_cursor += eff_gap;
            }
            let d = layout.desired[c.index()];
            let s = *tree.layout(c);
            // Cross axis: each child placed within the line's cross
            // extent via `place_axis`. Same rule as Stack cross —
            // Fill stretches to line_cross, Hug aligns per child.
            let (h_align, v_align) = resolved_axis_align(&s, parent_child_align);
            let cross_align = match axis {
                Axis::X => v_align,
                Axis::Y => h_align,
            };
            let (cross_size, cross_off) = place_axis(
                cross_align,
                axis.cross_sizing(s.size),
                axis.cross(d),
                line_cross,
                AutoBias::StretchIfFill,
            );
            let main_size = axis.main(d);
            let child_rect = axis.compose_rect(
                main_cursor,
                *cross_cursor + cross_off,
                main_size,
                cross_size,
            );
            layout.arrange(tree, c, child_rect);
            main_cursor += main_size;
        }
        *cross_cursor += line_cross;
        // Drop our line from the pool (capacity retained). Recursive
        // `layout.arrange` calls above may have temporarily extended
        // and re-truncated the pool past `line_end`; we ignore those
        // and reset to our depth's start.
        layout.wrap.pool.truncate(line_start);
    };

    // Walk all children: collapsed get zeroed at the cursor, active
    // children pack into the current line and flush on overflow.
    for c in tree.children(node) {
        if tree.is_collapsed(c) {
            // Anchor inside this layout's inner rect at the current
            // cursor. Position is stable; size is zero so there's no
            // visual or input contribution.
            zero_subtree(
                layout,
                tree,
                c,
                axis.compose_point(axis.main_v(inner.min), cross_cursor),
            );
            continue;
        }

        let d = layout.desired[c.index()];
        let m = axis.main(d);
        // Mirror the measure-side rule: Fill-on-cross children don't
        // contribute to `line_cross`. Without this the row stretches
        // to the WrapStack's inner cross (because Fill measured to
        // `cross_avail`), defeating per-row hug semantics.
        let x = if matches!(axis.cross_sizing(tree.layout(c).size), Sizing::Fill(_)) {
            0.0
        } else {
            axis.cross(d)
        };

        let candidate = if line_main > 0.0 {
            line_main + gap + m
        } else {
            m
        };
        if line_main > 0.0 && candidate > main_avail {
            place_line(
                layout,
                line_main,
                line_cross,
                &mut cross_cursor,
                &mut first_line,
            );
            layout.wrap.pool.push(c);
            line_main = m;
            line_cross = x;
        } else {
            layout.wrap.pool.push(c);
            line_main = candidate;
            line_cross = line_cross.max(x);
        }
    }
    place_line(
        layout,
        line_main,
        line_cross,
        &mut cross_cursor,
        &mut first_line,
    );
    layout.wrap.exit();
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
pub(super) fn intrinsic(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    main_axis: Axis,
    query_axis: Axis,
    req: LenReq,
    text: &mut TextMeasurer,
) -> f32 {
    let extras = tree.read_extras(node);
    let gap = extras.gap;
    if main_axis == query_axis {
        match req {
            LenReq::MinContent => {
                let mut floor = 0.0f32;
                for c in tree.children_active(node) {
                    floor = floor.max(layout.intrinsic(tree, c, query_axis, req, text));
                }
                floor
            }
            LenReq::MaxContent => {
                let mut total = 0.0f32;
                let mut count = 0usize;
                for c in tree.children_active(node) {
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
        for c in tree.children_active(node) {
            max = max.max(layout.intrinsic(tree, c, query_axis, req, text));
        }
        max
    }
}

#[cfg(test)]
mod tests;
