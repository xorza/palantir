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
use crate::primitives::{Justify, Rect, Size};
use crate::text::TextMeasurer;
use crate::tree::{NodeId, Tree};

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
        let x = axis.cross(d);

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

    // Same packing logic as `measure`, but this time we hold each line
    // open long enough to place its children. We re-walk children,
    // accumulating into a current-line buffer, then flush via
    // `place_line` on overflow / end-of-children.

    // Per-line scratch held across the children loop; each entry is
    // `(NodeId, desired_size)` for one child in the current line. Reset
    // when the line is flushed.
    let mut line: Vec<(NodeId, Size)> = Vec::new();
    let mut line_main = 0.0f32;
    let mut line_cross = 0.0f32;
    let mut cross_cursor = axis.cross_v(inner.min);
    let mut first_line = true;

    let place_line = |layout: &mut LayoutEngine,
                      line: &mut Vec<(NodeId, Size)>,
                      line_main: f32,
                      line_cross: f32,
                      cross_cursor: &mut f32,
                      first_line: &mut bool| {
        if line.is_empty() {
            return;
        }
        if !*first_line {
            *cross_cursor += line_gap;
        }
        *first_line = false;

        let count = line.len();
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
        for (idx, (c, d)) in line.iter().enumerate() {
            if idx > 0 {
                main_cursor += eff_gap;
            }
            let s = *tree.layout(*c);
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
                axis.cross(*d),
                line_cross,
                AutoBias::StretchIfFill,
            );
            let main_size = axis.main(*d);
            let child_rect = axis.compose_rect(
                main_cursor,
                *cross_cursor + cross_off,
                main_size,
                cross_size,
            );
            layout.arrange(tree, *c, child_rect);
            main_cursor += main_size;
        }
        *cross_cursor += line_cross;
        line.clear();
    };

    // Walk all children: collapsed get zeroed at the cursor, active
    // children pack into the current line and flush on overflow.
    for c in tree.children(node) {
        if tree.is_collapsed(c) {
            // Place at the line's start anchor — nothing visible, just a
            // stable anchor inside this layout's inner rect.
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
        let x = axis.cross(d);

        let candidate = if line_main > 0.0 {
            line_main + gap + m
        } else {
            m
        };
        if line_main > 0.0 && candidate > main_avail {
            place_line(
                layout,
                &mut line,
                line_main,
                line_cross,
                &mut cross_cursor,
                &mut first_line,
            );
            // Start new line with this child.
            line.push((c, d));
            line_main = m;
            line_cross = x;
        } else {
            line.push((c, d));
            line_main = candidate;
            line_cross = line_cross.max(x);
        }
    }
    place_line(
        layout,
        &mut line,
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
