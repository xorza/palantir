//! Per-flavor measure / arrange / content-derive helpers for the
//! three scroll layout modes (`ScrollV`, `ScrollH`, `ScrollXY`).
//!
//! Each scroll mode runs an underlying driver — `stack::measure` for
//! the single-axis variants, `zstack::measure` for both-axes — with
//! the panned axis fed `f32::INFINITY` so children report their full
//! natural extent. The viewport itself reports zero on the panned
//! axis so `resolve_desired` falls through to its own `Sizing`.
//!
//! `derive_content` mirrors these formulas for the measure-cache hit
//! path, where `desired` is restored from a snapshot but
//! `scroll_content` (sparse, not cached) needs to be recomputed.
//! Keep the formulas in lock-step with the measure ones.

use super::stack;
use super::zstack;
use super::{Axis, LayoutEngine};
use crate::primitives::size::Size;
use crate::text::TextMeasurer;
use crate::tree::element::LayoutMode;
use crate::tree::{NodeId, Tree};

/// `LayoutMode::ScrollV` measure: VStack of children with `available.h
/// = INF`. Returns the viewport's content extent (cross max + main
/// sum + gap) for storage; `measure_dispatch` strips the panned axis
/// before passing to `resolve_desired`.
pub(crate) fn measure_v(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    inner_avail: Size,
    text: &mut TextMeasurer,
) -> Size {
    let unbounded = Size::new(inner_avail.w, f32::INFINITY);
    stack::measure(layout, tree, node, unbounded, Axis::Y, text)
}

/// `LayoutMode::ScrollH` measure — mirror of [`measure_v`].
pub(crate) fn measure_h(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    inner_avail: Size,
    text: &mut TextMeasurer,
) -> Size {
    let unbounded = Size::new(f32::INFINITY, inner_avail.h);
    stack::measure(layout, tree, node, unbounded, Axis::X, text)
}

/// `LayoutMode::ScrollXY` measure: ZStack of children with both axes
/// unbounded. Children should size with `Hug` or `Fixed` — `Fill` has
/// no defined content axis and collapses to max-content under
/// `resolve_axis_size`'s INF-available rule.
pub(crate) fn measure_xy(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    text: &mut TextMeasurer,
) -> Size {
    zstack::measure(layout, tree, node, Size::INF, text)
}

/// Recompute `scroll_content` for `node` from the just-restored
/// children's `desired`. Called from the measure-cache hit path,
/// where the column isn't snapshotted but the inputs are. Formulas
/// must match the underlying drivers' output:
///   - ScrollV / stack(Y): `Σ child.h + (n-1)·gap`, `max child.w`
///   - ScrollH / stack(X): mirror
///   - ScrollXY / zstack:  `max child.w`, `max child.h`
pub(crate) fn derive_content(
    tree: &Tree,
    desired: &[Size],
    node: NodeId,
    mode: LayoutMode,
) -> Size {
    let gap = tree.read_extras(node).gap;
    let mut sum_w = 0.0_f32;
    let mut sum_h = 0.0_f32;
    let mut max_w = 0.0_f32;
    let mut max_h = 0.0_f32;
    let mut count = 0usize;
    for c in tree.children_active(node) {
        let d = desired[c.index()];
        sum_w += d.w;
        sum_h += d.h;
        max_w = max_w.max(d.w);
        max_h = max_h.max(d.h);
        count += 1;
    }
    let gap_total = if count > 1 {
        gap * (count - 1) as f32
    } else {
        0.0
    };
    match mode {
        LayoutMode::ScrollV => Size::new(max_w, sum_h + gap_total),
        LayoutMode::ScrollH => Size::new(sum_w + gap_total, max_h),
        LayoutMode::ScrollXY => Size::new(max_w, max_h),
        _ => panic!("derive_content called with non-scroll mode {mode:?}"),
    }
}

#[cfg(test)]
mod tests;
