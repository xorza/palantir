use super::support::{
    AutoBias, children_max_intrinsic, place_axis, resolved_axis_align, zero_subtree,
};
use super::{Axis, LayoutEngine, LenReq};
use crate::element::LayoutCore;
use crate::primitives::{Align, AxisAlign, Justify, Rect, Size, Sizing};
use crate::text::TextMeasurer;
use crate::tree::{NodeId, Tree};

/// Cross-axis alignment of a child, picked from the shared two-axis
/// `resolved_axis_align` so HStack/VStack share the cascade rule with
/// ZStack/Grid. The unused main axis is computed and discarded — cheap.
fn cross_align(axis: Axis, child: &LayoutCore, parent_child_align: Align) -> AxisAlign {
    let (h, v) = resolved_axis_align(child, parent_child_align);
    match axis {
        // HStack: cross = vertical
        Axis::X => v,
        // VStack: cross = horizontal
        Axis::Y => h,
    }
}

pub(super) fn measure(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    inner: Size,
    axis: Axis,
    text: &mut TextMeasurer,
) -> Size {
    let gap = tree.read_extras(node).gap;
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
    for c in tree.children_active(node) {
        count += 1;
        let l = tree.layout(c);
        if let Sizing::Fill(w) = axis.main_sizing(l.size) {
            total_weight += w;
            continue;
        }
        let d = layout.measure(tree, c, axis.compose_size(f32::INFINITY, cross_avail), text);
        sum_non_fill_main += axis.main(d);
        max_cross = max_cross.max(axis.cross(d));
    }
    let total_gap = gap * count.saturating_sub(1) as f32;

    // Pass 2: measure Fill children. If the stack's main axis is finite
    // each Fill child gets its resolved share floored at `MinContent`
    // and capped by `max_size`; on a Hug stack (INF main) Fill children
    // measure at INF main and report their natural width (matches the
    // "Hug stack hugs to children's natural widths" rule).
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
        let leftover = if main_finite {
            (axis.main(inner) - sum_non_fill_main - total_gap).max(0.0)
        } else {
            0.0
        };
        for c in tree.children_active(node) {
            let Sizing::Fill(w) = axis.main_sizing(tree.layout(c).size) else {
                continue;
            };
            let main_avail = if main_finite {
                let cap = axis.main(tree.read_extras(c).max_size);
                let target = (leftover * w / total_weight).min(cap);
                // Floor at min-content so wrap text doesn't break inside
                // a word (it overflows the slot instead — same rule the
                // leaf reshape branch in shape_text follows).
                let floor = layout.intrinsic(tree, c, axis, LenReq::MinContent, text);
                target.max(floor)
            } else {
                f32::INFINITY
            };
            let d = layout.measure(tree, c, axis.compose_size(main_avail, cross_avail), text);
            fill_main += axis.main(d);
            max_cross = max_cross.max(axis.cross(d));
        }
    }

    axis.compose_size(sum_non_fill_main + fill_main + total_gap, max_cross)
}

pub(super) fn arrange(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    inner: Rect,
    axis: Axis,
) {
    let extras = tree.read_extras(node);
    let (gap, justify, parent_child_align) = (extras.gap, extras.justify, extras.child_align);

    // Sum desired along main axis for non-Fill children; collect Fill weights.
    // Fill siblings split the remaining space proportionally (WPF Star semantics)
    // independent of their intrinsic content size.
    let mut sum_main_desired = 0.0f32;
    let mut total_weight = 0.0f32;
    let mut count = 0usize;
    for c in tree.children_active(node) {
        let l = tree.layout(c);
        if let Sizing::Fill(weight) = axis.main_sizing(l.size) {
            total_weight += weight;
        } else {
            sum_main_desired += axis.main(layout.scratch.desired[c.index()]);
        }
        count += 1;
    }
    let total_gap = gap * count.saturating_sub(1) as f32;

    let main_total = axis.main(inner.size);
    let cross = axis.cross(inner.size);
    let leftover = (main_total - sum_main_desired - total_gap).max(0.0);

    // `justify` distributes any unused main-axis space. With Fill children
    // present, leftover is consumed by Fill weights → justify is a no-op
    // (degrade to Start / original gap).
    let (start_offset, effective_gap) = if total_weight > 0.0 {
        (0.0, gap)
    } else {
        match justify {
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
        }
    };

    let cross_min = axis.cross_v(inner.min);
    let mut cursor = axis.main_v(inner.min) + start_offset;
    let mut first = true;

    for c in tree.children(node) {
        if tree.is_collapsed(c) {
            zero_subtree(layout, tree, c, axis.compose_point(cursor, cross_min));
            continue;
        }
        let s = *tree.layout(c);
        let d = layout.scratch.desired[c.index()];
        if !first {
            cursor += effective_gap;
        }
        first = false;

        let main_sizing = axis.main_sizing(s.size);
        let main_size = match main_sizing {
            Sizing::Fill(weight) if total_weight > 0.0 => leftover * (weight / total_weight),
            _ => axis.main(d),
        };

        let cross_align = cross_align(axis, &s, parent_child_align);
        let cross_sizing = axis.cross_sizing(s.size);
        let cross_desired = axis.cross(d);
        let (cross_size, cross_offset) = place_axis(
            cross_align,
            cross_sizing,
            cross_desired,
            cross,
            AutoBias::StretchIfFill,
        );

        let child_rect = axis.compose_rect(cursor, cross_min + cross_offset, main_size, cross_size);
        layout.arrange(tree, c, child_rect);
        cursor += main_size;
    }
}

/// Intrinsic size of a stack on `query_axis` under `req`. When the query
/// axis matches the stack's `main_axis`, sum children's intrinsic on
/// that axis plus gaps; otherwise (cross axis), max over children.
pub(super) fn intrinsic(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    main_axis: Axis,
    query_axis: Axis,
    req: LenReq,
    text: &mut TextMeasurer,
) -> f32 {
    let gap = tree.read_extras(node).gap;
    if main_axis == query_axis {
        let mut total = 0.0_f32;
        let mut count = 0_usize;
        for c in tree.children_active(node) {
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
