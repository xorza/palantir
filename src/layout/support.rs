//! Cross-driver helpers shared between `stack`, `zstack`, `canvas`, and
//! `grid`. Pure layout primitives — no engine state aside from the
//! `LayoutEngine` references threaded through where needed for intrinsic
//! caching and result writing.

use super::{Axis, LayoutEngine, LenReq};
use crate::layout::types::{align::Align, align::AxisAlign, sizing::Sizes, sizing::Sizing};
use crate::primitives::{rect::Rect, size::Size};
use crate::shape::{Shape, TextWrap};
use crate::text::TextMeasurer;
use crate::tree::element::LayoutCore;
use crate::tree::{NodeId, Tree};
use glam::Vec2;

/// Iterate `(text, font_size_px, wrap)` for every `Shape::Text` on a
/// leaf. Single source of truth for the layout-side leaf walk —
/// `mod.rs::leaf_content_size` drives wrap shaping, `intrinsic::leaf`
/// drives the unbounded content axis. Filtering and destructuring
/// happen here so neither side can drift on which shape variants
/// contribute to size.
pub(crate) fn leaf_text_shapes(
    tree: &Tree,
    node: NodeId,
) -> impl Iterator<Item = (&str, f32, TextWrap)> {
    tree.shapes_of(node).iter().filter_map(|s| match s {
        Shape::Text {
            text,
            font_size_px,
            wrap,
            ..
        } => Some((text.as_str(), *font_size_px, *wrap)),
        _ => None,
    })
}

/// Resolve a node's outer slot size on one axis, given its sizing
/// policy, hug-content size (margin-inclusive: content+padding+margin),
/// parent-supplied available, own margin, and clamps. Each branch
/// derives a *rendered* size (margin-exclusive) by subtracting margin,
/// clamps once, then re-adds margin at the end so the return is
/// margin-inclusive too. The margin round-trip exists so callers don't
/// have to special-case Fixed (which doesn't read `hug_with_margin`)
/// vs Hug/Fill (which do).
///
/// Also reused by `intrinsic::compute` with `available = INFINITY`,
/// which collapses Fill to its content size — the parent-independent
/// rule for intrinsic queries (CSS Grid `1fr`-in-auto-context).
pub(crate) fn resolve_axis_size(
    s: Sizing,
    hug_with_margin: f32,
    available: f32,
    margin: f32,
    min: f32,
    max: f32,
) -> f32 {
    let rendered = match s {
        Sizing::Fixed(v) => v,
        Sizing::Hug => hug_with_margin - margin,
        Sizing::Fill(_) => {
            // Fill in an unconstrained axis collapses to max-content
            // (matches CSS Grid: a `1fr` track with `width: auto` parent
            // resolves to its content size, not infinity).
            let outer = if available.is_finite() {
                available
            } else {
                hug_with_margin
            };
            outer - margin
        }
    };
    rendered.max(0.0).clamp(min, max) + margin
}

/// Set this node and every descendant to a zero-size rect anchored at
/// `anchor`. Walks the contiguous pre-order span `[node, subtree_end[node])`
/// directly — no recursion, no child cursors.
pub(crate) fn zero_subtree(layout: &mut LayoutEngine, tree: &Tree, node: NodeId, anchor: Vec2) {
    let zero = Rect {
        min: anchor,
        size: Size::ZERO,
    };
    let start = node.index();
    let end = tree.subtree_end[start] as usize;
    for i in start..end {
        layout.result.set_rect(NodeId(i as u32), zero);
    }
}

/// Max over non-collapsed children's outer intrinsic on `axis`. Used by
/// drivers whose own size on an axis is "the largest child wants this much"
/// (ZStack on either axis, Stack on the cross axis). Canvas can't use it
/// because it adds child position to the contribution.
pub(crate) fn children_max_intrinsic(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    axis: Axis,
    req: LenReq,
    text: &mut TextMeasurer,
) -> f32 {
    let mut m = 0.0f32;
    for c in tree.children_active(node) {
        m = m.max(layout.intrinsic(tree, c, axis, req, text));
    }
    m
}

/// Per-axis available size to pass to children of a panel that sizes per its
/// own `Sizing` on each axis: pass `inner_avail` on Fill/Fixed axes (children
/// see the committed slot), `INFINITY` on Hug axes (avoids recursive sizing).
/// Used by ZStack and Canvas. Stack uses a different rule (always INF on main).
///
/// `INF` here is *height-given-width* via measure, not an intrinsic-replaceable
/// sentinel. Replacing it with `intrinsic(MaxContent)` looks equivalent for
/// leaves but is wrong for nested containers whose main-axis size depends on
/// cross-axis (Grid with wrapping cells, etc.) — intrinsic queries the
/// unbounded shape, while INF-measure runs the child's full layout under the
/// committed cross.
pub(crate) fn child_avail_per_axis_hug(size: Sizes, inner_avail: Size) -> Size {
    Size::new(
        if matches!(size.w, Sizing::Hug) {
            f32::INFINITY
        } else {
            inner_avail.w
        },
        if matches!(size.h, Sizing::Hug) {
            f32::INFINITY
        } else {
            inner_avail.h
        },
    )
}

/// How `place_axis` interprets `AxisAlign::Auto`.
#[derive(Copy, Clone, PartialEq, Eq)]
pub(crate) enum AutoBias {
    /// Stack/ZStack: Auto stretches only when the child is `Sizing::Fill`.
    StretchIfFill,
    /// Grid: Auto stretches unconditionally (WPF cell default).
    AlwaysStretch,
}

/// Resolved horizontal/vertical alignment after the cascade.
pub(crate) struct AxisAlignPair {
    pub(crate) h: AxisAlign,
    pub(crate) v: AxisAlign,
}

/// Per-axis placement: chosen extent + offset within the parent's inner span.
pub(crate) struct AxisPlacement {
    pub(crate) size: f32,
    pub(crate) offset: f32,
}

/// Two-axis placement: chosen size + offset within the parent's inner rect.
pub(crate) struct Placement {
    pub(crate) size: Size,
    pub(crate) offset: Vec2,
}

/// Resolve a child's alignment on both axes: child's own value if not `Auto`,
/// else the parent's `child_align` for that axis. Single source of truth for
/// the alignment cascade — every layout (stack, grid, zstack) calls this so
/// they can't drift. Stack discards the unused axis; the cost is two enum
/// matches per child per frame.
pub(crate) fn resolved_axis_align(child: &LayoutCore, parent_child_align: Align) -> AxisAlignPair {
    let a = child.align;
    AxisAlignPair {
        h: a.halign().or(parent_child_align.halign()).to_axis(),
        v: a.valign().or(parent_child_align.valign()).to_axis(),
    }
}

/// Compute size + offset along one axis given the child's alignment, its
/// declared sizing, intrinsic desired size, and the inner span available.
/// Used for stack cross-axis, ZStack per-axis, and Grid per-cell placement.
/// `bias` selects the per-driver `AxisAlign::Auto` rule (see `AutoBias`).
pub(crate) fn place_axis(
    align: AxisAlign,
    sizing: Sizing,
    desired: f32,
    inner: f32,
    bias: AutoBias,
) -> AxisPlacement {
    let stretch = matches!(align, AxisAlign::Stretch)
        || matches!(align, AxisAlign::Auto)
            && (matches!(bias, AutoBias::AlwaysStretch) || matches!(sizing, Sizing::Fill(_)));
    let size = if stretch { inner } else { desired };
    let offset = match align {
        AxisAlign::Center => ((inner - size) * 0.5).max(0.0),
        AxisAlign::End => (inner - size).max(0.0),
        _ => 0.0,
    };
    AxisPlacement { size, offset }
}

/// Resolve a child's two-axis size + offset inside `inner`, applying the
/// alignment cascade and the per-driver `AutoBias` rule. Used by ZStack and
/// Grid arrange — both place each child independently per axis using the
/// same rule. Stack does cross-axis placement only (different main-axis
/// rule) so it still calls `place_axis` directly on cross.
pub(crate) fn place_two_axis(
    child: &LayoutCore,
    parent_child_align: Align,
    desired: Size,
    inner: Size,
    bias: AutoBias,
) -> Placement {
    let AxisAlignPair {
        h: h_align,
        v: v_align,
    } = resolved_axis_align(child, parent_child_align);
    let x = place_axis(h_align, child.size.w, desired.w, inner.w, bias);
    let y = place_axis(v_align, child.size.h, desired.h, inner.h, bias);
    Placement {
        size: Size::new(x.size, y.size),
        offset: Vec2::new(x.offset, y.offset),
    }
}
