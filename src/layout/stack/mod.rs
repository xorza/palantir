pub use super::axis::Axis;
use super::{AutoBias, LayoutEngine, LenReq, place_axis, resolved_axis_align, zero_subtree};
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
    // Pass 1: WPF intrinsic trick — INF on main, finite on cross. Every
    // child reports its intrinsic main size. For Fill children that's
    // their natural (max-content) main; we'll re-measure them in pass 2.
    let child_avail = axis.compose_size(f32::INFINITY, axis.cross(inner));
    let gap = tree.read_extras(node).gap;

    let mut total_main = 0.0f32;
    let mut max_cross = 0.0f32;
    let mut count = 0usize;
    let mut total_weight = 0.0f32;
    let mut sum_non_fill_main = 0.0f32;
    // Skip collapsed children outright: `LayoutEngine.desired` is reset to
    // `Size::ZERO` for every node at the top of `run`, so a collapsed
    // child's `desired` is already correct without a measure call.
    for c in tree.children(node) {
        if tree.is_collapsed(c) {
            continue;
        }
        let d = layout.measure(tree, c, child_avail, text);
        let l = tree.layout(c);
        if let Sizing::Fill(w) = axis.main_sizing(l.size) {
            assert!(w > 0.0, "Sizing::Fill weight must be positive");
            total_weight += w;
        } else {
            sum_non_fill_main += axis.main(d);
        }
        total_main += axis.main(d);
        max_cross = max_cross.max(axis.cross(d));
        count += 1;
    }
    let total_gap = gap * count.saturating_sub(1) as f32;

    // Step C: if the stack has a finite main-axis size and Fill children,
    // re-measure each Fill child at its resolved Fill share so wrap text
    // shapes correctly. Without this, Fill children are committed to their
    // natural (max-content) widths from pass 1, and arrange clamps them
    // into a smaller slot — text overflows the slot visually.
    //
    // Hug stacks (`inner.main = INF`) skip this branch — Fill children
    // there fall back to natural width as before, matching the existing
    // "Hug stack hugs to children's natural widths including Fill" rule.
    //
    // Soundness of pass 2: the `axis.main(inner)` we use here as the
    // budget must equal the `axis.main(inner)` the matching `arrange`
    // call sees — otherwise Fill children's wrap text is shaped against
    // the wrong width. It does, because the Stack's outer main size is a
    // deterministic function of (its own `Sizing` + parent-supplied
    // `available`) via `resolve_axis_size`, and the parent passes the
    // same `available` to `measure` that it later derives `slot.size`
    // from for `arrange`. Any future driver that clamps a child's slot
    // *between* its own measure and arrange would break this — adding a
    // new layout mode? Re-derive `inner` here from the post-measure
    // `desired` instead of trusting the parameter.
    if total_weight > 0.0 && axis.main(inner).is_finite() {
        let leftover = (axis.main(inner) - sum_non_fill_main - total_gap).max(0.0);
        // Restart `total_main` from `sum_non_fill_main`: pass-1 also
        // accumulated each Fill child's natural main, which is now stale.
        // Non-Fill mains are still correct, so we keep them and only add
        // Fill children's resolved mains in the loop (adding non-Fill's
        // desired here too would double-count).
        //
        // `max_cross` resets — wrap text in Fill children grows in cross
        // when re-measured at a narrower main, so we re-max from scratch
        // across all live children.
        total_main = sum_non_fill_main;
        max_cross = 0.0;
        for c in tree.children(node) {
            if tree.is_collapsed(c) {
                continue;
            }
            let l = tree.layout(c);
            let new_d = if let Sizing::Fill(w) = axis.main_sizing(l.size) {
                let extras = tree.read_extras(c);
                let cap = axis.main(extras.max_size);
                let target = (leftover * w / total_weight).min(cap);
                // Floor at min-content so wrap text doesn't break inside
                // a word (it overflows the slot instead — same rule the
                // leaf reshape branch in shape_text follows).
                let floor = layout.intrinsic(tree, c, axis, LenReq::MinContent, text);
                let resolved = target.max(floor);
                let new_avail = axis.compose_size(resolved, axis.cross(inner));
                let new_d = layout.measure(tree, c, new_avail, text);
                total_main += axis.main(new_d);
                new_d
            } else {
                layout.desired(c)
            };
            max_cross = max_cross.max(axis.cross(new_d));
        }
    }

    total_main += total_gap;
    axis.compose_size(total_main, max_cross)
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
    for c in tree.children(node) {
        if tree.is_collapsed(c) {
            continue;
        }
        let l = tree.layout(c);
        if let Sizing::Fill(weight) = axis.main_sizing(l.size) {
            assert!(weight > 0.0, "Sizing::Fill weight must be positive");
            total_weight += weight;
        } else {
            sum_main_desired += axis.main(layout.desired(c));
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
        let d = layout.desired(c);
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
        for c in tree.children(node) {
            if tree.is_collapsed(c) {
                continue;
            }
            total += layout.intrinsic(tree, c, query_axis, req, text);
            count += 1;
        }
        total + gap * count.saturating_sub(1) as f32
    } else {
        let mut max = 0.0_f32;
        for c in tree.children(node) {
            if tree.is_collapsed(c) {
                continue;
            }
            max = max.max(layout.intrinsic(tree, c, query_axis, req, text));
        }
        max
    }
}

#[cfg(test)]
mod tests;
