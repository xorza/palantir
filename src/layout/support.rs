//! Cross-driver helpers shared between `stack`, `zstack`, `canvas`, and
//! `grid`. Pure layout primitives — no engine state aside from the
//! `LayoutEngine` references threaded through where needed for intrinsic
//! caching and result writing.

use super::axis::Axis;
use super::intrinsic::LenReq;
use super::layoutengine::LayoutEngine;
use crate::forest::element::LayoutCore;
use crate::forest::shapes::record::ShapeRecord;
use crate::forest::tree::{NodeId, Tree};
use crate::layout::Layout;
use crate::layout::types::align::HAlign;
use crate::layout::types::{
    align::Align, align::AxisAlign, justify::Justify, sizing::Sizes, sizing::Sizing,
};
use crate::primitives::{rect::Rect, size::Size};
use crate::shape::TextWrap;
use crate::text::{FontFamily, TextShaper};
use glam::Vec2;

/// Read-only context every layout method threads through the
/// measure / arrange / intrinsic recursion. Bundles the text shaper
/// with the per-frame text-byte arena (the byte slice that
/// `ShapeRecord::Text::text`'s `InternedStr::Interned` span resolves
/// against — see [`InternedStr::as_str`](crate::InternedStr::as_str)).
/// Single parameter slot instead of `(text_bytes: &str, text:
/// &TextShaper)` everywhere keeps driver signatures readable.
#[derive(Copy, Clone)]
pub(crate) struct TextCtx<'a> {
    pub(crate) bytes: &'a str,
    pub(crate) shaper: &'a TextShaper,
}

/// One `ShapeRecord::Text` worth of layout-side inputs. Yielded by
/// [`leaf_text_shapes`]; named so the fields aren't a tuple.
pub(crate) struct LeafTextShape<'a> {
    pub(crate) text: &'a str,
    pub(crate) font_size_px: f32,
    pub(crate) line_height_px: f32,
    pub(crate) wrap: TextWrap,
    pub(crate) family: FontFamily,
    /// Horizontal alignment from `Shape::Text.align`. Cosmic-text
    /// bakes per-line offsets into the shaped buffer when wrap is on,
    /// so the layout pass has to thread this all the way down to
    /// `shape_wrap` (and into `TextCacheKey`) — two shapes with
    /// identical text/size/wrap but different halign aren't
    /// interchangeable.
    pub(crate) halign: HAlign,
}

/// Iterate every `ShapeRecord::Text` on a leaf. Single source of truth for
/// the layout-side leaf walk — `mod.rs::leaf_content_size` drives wrap
/// shaping, `intrinsic::leaf` drives the unbounded content axis.
/// Filtering and destructuring happen here so neither side can drift
/// on which shape variants contribute to size.
pub(crate) fn leaf_text_shapes<'a>(
    tree: &'a Tree,
    tc: &TextCtx<'a>,
    node: NodeId,
) -> impl Iterator<Item = LeafTextShape<'a>> {
    // Direct slice into `tree.shapes` for `node`. Leaves have no children,
    // so the `records.shape_span()[i]` span is exactly the leaf's own direct
    // shapes — contiguous, no child boundaries to skip.
    assert_eq!(
        tree.records.subtree_end()[node.index()],
        node.0 + 1,
        "leaf_text_shapes called on non-leaf node {node:?}",
    );
    let span = tree.records.shape_span()[node.index()];
    let lo = span.start as usize;
    let hi = lo + span.len as usize;
    tree.shapes.records[lo..hi]
        .iter()
        .filter_map(move |s| match s {
            ShapeRecord::Text {
                text,
                font_size_px,
                line_height_px,
                wrap,
                family,
                align,
                ..
            } => Some(LeafTextShape {
                text: text.as_str(tc.bytes),
                font_size_px: *font_size_px,
                line_height_px: *line_height_px,
                wrap: *wrap,
                family: *family,
                halign: align.halign(),
            }),
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
///
/// Per-axis inputs for [`resolve_axis_size`]. Bundles the seven
/// numbers + `Sizing` into one struct so the call site reads as
/// "given this axis context, resolve the outer size" rather than a
/// 7-arg parameter cliff. Margin-inclusive convention: `available`
/// and the returned value both include the node's own margin on this
/// axis; `hug_with_margin` is `content + padding + margin`.
pub(crate) struct AxisCtx {
    pub sizing: Sizing,
    pub hug_with_margin: f32,
    pub available: f32,
    pub intrinsic_min: f32,
    pub margin: f32,
    pub min: f32,
    pub max: f32,
}

/// **Flex-shrink semantics with min-content floor:** Hug clamps down
/// to fit `available`; Fill consumes `available` exactly. Both axes
/// shrink with parent down to `intrinsic_min` — the largest
/// non-shrinkable descendant on this axis (Fixed widget extents,
/// explicit `min_size`, longest-unbreakable-word for wrapping text).
/// This matches CSS Flexbox's default `min-width: auto` for flex
/// items: a flex item shrinks down to min-content, then stops.
///
/// The only ways desired can exceed `available` are
/// `intrinsic_min > available` (rigid descendant doesn't fit), an
/// explicit `min_size` floor, or `Sizing::Fixed(v)`. When that
/// happens the child's rect overflows its slot; downstream
/// (cascade/composer/backend) tolerates it, same as the
/// root-vs-surface overflow.
///
/// `Fill` on an unconstrained axis (intrinsic queries with
/// `available = INFINITY`) collapses to its content size — matches
/// CSS Grid's `1fr` track in an auto-context parent.
pub(crate) fn resolve_axis_size(ctx: AxisCtx) -> f32 {
    let content = ctx.hug_with_margin - ctx.margin;
    let rendered = match ctx.sizing {
        Sizing::Fixed(v) => v,
        Sizing::Hug => {
            if ctx.available.is_finite() {
                content
                    .min(ctx.available - ctx.margin)
                    .max(ctx.intrinsic_min - ctx.margin)
            } else {
                content
            }
        }
        Sizing::Fill(_) => {
            if ctx.available.is_finite() {
                (ctx.available - ctx.margin).max(ctx.intrinsic_min - ctx.margin)
            } else {
                content
            }
        }
    };
    rendered.max(0.0).clamp(ctx.min, ctx.max) + ctx.margin
}

/// Set this node and every descendant to a zero-size rect anchored at
/// `anchor`. Walks the contiguous pre-order span `[node, subtree_end[node])`
/// directly — no recursion, no child cursors.
pub(crate) fn zero_subtree(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    anchor: Vec2,
    out: &mut Layout,
) {
    let zero = Rect {
        min: anchor,
        size: Size::ZERO,
    };
    let start = node.index();
    let end = (tree.records.subtree_end()[start]) as usize;
    out[layout.active_layer].rect[start..end].fill(zero);
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
    tc: &TextCtx<'_>,
) -> f32 {
    let mut m = 0.0f32;
    for c in tree.active_children(node) {
        m = m.max(layout.intrinsic(tree, c, axis, req, tc));
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
        if matches!(size.w(), Sizing::Hug) {
            f32::INFINITY
        } else {
            inner_avail.w
        },
        if matches!(size.h(), Sizing::Hug) {
            f32::INFINITY
        } else {
            inner_avail.h
        },
    )
}

/// Main-axis offset + effective inter-child gap for one row of
/// `justify`-distributed children. Single source of truth for Stack and
/// WrapStack — keeps SpaceBetween / SpaceAround degeneracy rules
/// (count < 2 / count < 1) in one place.
pub(crate) struct JustifyOffsets {
    pub(crate) start: f32,
    pub(crate) gap: f32,
}

pub(crate) fn justify_offsets(
    justify: Justify,
    leftover: f32,
    gap: f32,
    count: usize,
) -> JustifyOffsets {
    match justify {
        Justify::Start => JustifyOffsets { start: 0.0, gap },
        Justify::Center => JustifyOffsets {
            start: leftover * 0.5,
            gap,
        },
        Justify::End => JustifyOffsets {
            start: leftover,
            gap,
        },
        Justify::SpaceBetween if count > 1 => JustifyOffsets {
            start: 0.0,
            gap: gap + leftover / (count - 1) as f32,
        },
        Justify::SpaceAround if count > 0 => {
            let extra = leftover / count as f32;
            JustifyOffsets {
                start: extra * 0.5,
                gap: gap + extra,
            }
        }
        // Fewer than 2 / 1 children → fallback to Start.
        Justify::SpaceBetween | Justify::SpaceAround => JustifyOffsets { start: 0.0, gap },
    }
}

/// Measure children of a per-axis-hug panel (ZStack / Canvas). Per
/// active child, calls `layout.measure` against the per-axis-hug
/// `child_avail`, then folds the child's contribution (size + offset
/// from `contrib`) into a per-axis max. Drivers differ only in
/// whether they add a positional offset.
pub(crate) fn measure_per_axis_hug(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    inner_avail: Size,
    tc: &TextCtx<'_>,
    out: &mut Layout,
    mut contrib: impl FnMut(&Tree, NodeId, Size) -> Size,
) -> Size {
    let style = tree.records.layout()[node.index()];
    let child_avail = child_avail_per_axis_hug(style.size, inner_avail);
    let mut max_w = 0.0f32;
    let mut max_h = 0.0f32;
    for c in tree.active_children(node) {
        let d = layout.measure(tree, c, child_avail, tc, out);
        let cont = contrib(tree, c, d);
        max_w = max_w.max(cont.w);
        max_h = max_h.max(cont.h);
    }
    Size::new(max_w, max_h)
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

/// Resolve a child's alignment on both axes: child's own value if not `Auto`,
/// else the parent's `child_align` for that axis. Single source of truth for
/// the alignment cascade — every layout (stack, grid, zstack) calls this so
/// they can't drift. Stack discards the unused axis; the cost is two enum
/// matches per child per frame.
pub(crate) fn resolved_axis_align(child: &LayoutCore, parent_child_align: Align) -> AxisAlignPair {
    let a = child.align();
    AxisAlignPair {
        h: a.halign().or(parent_child_align.halign()).to_axis(),
        v: a.valign().or(parent_child_align.valign()).to_axis(),
    }
}

/// Compute size + offset along one axis given the child's alignment, its
/// declared sizing, intrinsic desired size, and the inner span available.
/// Used for stack cross-axis, ZStack per-axis, and Grid per-cell placement.
///
/// `Auto` stretches only when the child is `Sizing::Fill` — the default
/// for stack / wrapstack / zstack. Grid wants `Auto` to stretch
/// unconditionally (WPF cell default); it pre-substitutes `Auto →
/// Stretch` at its call site rather than threading a per-driver flag
/// here.
pub(crate) fn place_axis(
    align: AxisAlign,
    sizing: Sizing,
    desired: f32,
    inner: f32,
) -> AxisPlacement {
    let stretch = matches!(align, AxisAlign::Stretch)
        || matches!(align, AxisAlign::Auto) && matches!(sizing, Sizing::Fill(_));
    let size = if stretch { inner } else { desired };
    let offset = match align {
        AxisAlign::Center => ((inner - size) * 0.5).max(0.0),
        AxisAlign::End => (inner - size).max(0.0),
        _ => 0.0,
    };
    AxisPlacement { size, offset }
}

/// Cross-axis placement for a child of a main-axis stack (Stack /
/// WrapStack). Resolves the alignment cascade, picks the cross axis
/// from the resolved (h, v) pair, and runs `place_axis` against the
/// child's cross sizing + desired + the parent's cross extent. Single
/// source of truth so the cascade rule can't drift between Stack and
/// WrapStack.
pub(crate) fn cross_place(
    main_axis: Axis,
    child: &LayoutCore,
    parent_child_align: Align,
    desired: Size,
    inner_cross: f32,
) -> AxisPlacement {
    let AxisAlignPair { h, v } = resolved_axis_align(child, parent_child_align);
    let cross_align = match main_axis {
        Axis::X => v,
        Axis::Y => h,
    };
    place_axis(
        cross_align,
        main_axis.cross_sizing(child.size),
        main_axis.cross(desired),
        inner_cross,
    )
}
