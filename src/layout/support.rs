//! Cross-driver helpers shared between `stack`, `zstack`, `canvas`, and
//! `grid`. Pure layout primitives — no engine state aside from the
//! `LayoutEngine` references threaded through where needed for intrinsic
//! caching and result writing.

use crate::layout::axis::Axis;
use crate::layout::engine::LayoutEngine;
use crate::layout::intrinsic::{IntrinsicQuery, IntrinsicRange};
use crate::layout::pass::LayoutPass;
use crate::layout::types::align::HAlign;
use crate::layout::types::{align::Align, align::AxisAlign, justify::Justify, sizing::Sizing};
use crate::primitives::interned_str::InternedText;
use crate::primitives::size::Size;
use crate::scene::node::columns::{BoundsExtras, LayoutCore};
use crate::scene::shapes::record::ShapeRecord;
use crate::scene::tree::Tree;
use crate::scene::tree::iter::TreeItem;
use crate::scene::tree::record::NodeId;
use crate::text::glyph_font::GlyphFont;
use crate::text::key::TextShapeKey;
use crate::text::request::TextShapeRequest;
use crate::text::wrap::TextWrap;

/// One `ShapeRecord::Text` worth of layout-side inputs. Yielded by
/// [`leaf_text_shapes`] and [`container_text_shapes`]; named so the fields
/// aren't a tuple.
#[derive(Debug)]
pub(super) struct TextShapeInput<'a> {
    pub(super) ordinal: u16,
    pub(super) text: &'a str,
    /// Content hash retained on the [`RecordedText`] at record time —
    /// [`Self::shape_request`] reuses it so shaping passes don't rescan
    /// the source bytes.
    ///
    /// [`RecordedText`]: crate::primitives::interned_str::RecordedText
    pub(super) text_hash: u64,
    /// The face this shapes in, carried whole from
    /// [`ShapeRecord::Text`] so the record and the shaper cannot
    /// disagree about what four separate fields meant.
    pub(super) font: GlyphFont,
    pub(super) wrap: TextWrap,
    /// Horizontal alignment from `Shape::Text.align`. Cosmic-text
    /// bakes per-line offsets into the shaped buffer when wrap is on,
    /// so the layout pass has to thread this all the way down to
    /// `TextSystem::measure` (and into `TextShapeKey`) — two shapes with
    /// identical text/size/wrap but different halign aren't
    /// interchangeable.
    pub(super) halign: HAlign,
}

impl<'a> TextShapeInput<'a> {
    pub(super) fn shape_request(&self) -> TextShapeRequest<'a> {
        TextShapeRequest::for_key(
            self.text,
            TextShapeKey::unbounded(self.text_hash, self.font),
        )
    }
}

/// Iterate every `ShapeRecord::Text` on a leaf. Single source of truth for
/// the layout-side leaf walk — `LayoutEngine::measure_dispatch` drives wrap
/// shaping, `intrinsic::leaf` drives the unbounded content axis.
/// Filtering and destructuring happen here so neither side can drift
/// on which shape variants contribute to size.
pub(super) fn leaf_text_shapes<'a>(
    tree: &'a Tree,
    interned_text: &'a InternedText<'_>,
    node: NodeId,
) -> impl Iterator<Item = TextShapeInput<'a>> {
    // Direct slice into `tree.shapes` for `node`. Leaves have no children,
    // so the `records.shape_span()[i]` span is exactly the leaf's own direct
    // shapes — contiguous, no child boundaries to skip.
    debug_assert_eq!(
        tree.subtree_end_of(node.idx()),
        node.0 + 1,
        "leaf_text_shapes called on non-leaf node {node:?}",
    );
    let span = tree.records.shape_span()[node.idx()];
    let lo = span.start as usize;
    let hi = lo + span.len as usize;
    text_shape_inputs(tree.shapes.records[lo..hi].iter(), interned_text)
}

/// Iterate the direct text shapes on a container, skipping text belonging to
/// descendant nodes while preserving this node's within-owner record order.
pub(super) fn container_text_shapes<'a>(
    tree: &'a Tree,
    interned_text: &'a InternedText<'_>,
    node: NodeId,
) -> impl Iterator<Item = TextShapeInput<'a>> {
    text_shape_inputs(
        tree.tree_items(node).filter_map(|item| match item {
            TreeItem::ShapeRecord(_, shape) => Some(shape),
            TreeItem::Child(_) => None,
        }),
        interned_text,
    )
}

fn text_shape_inputs<'a>(
    shapes: impl Iterator<Item = &'a ShapeRecord> + 'a,
    interned_text: &'a InternedText<'_>,
) -> impl Iterator<Item = TextShapeInput<'a>> + 'a {
    let mut ordinal = 0;
    shapes.filter_map(move |shape| {
        let input = text_shape_input(shape, interned_text, ordinal)?;
        ordinal += 1;
        Some(input)
    })
}

fn text_shape_input<'a>(
    shape: &'a ShapeRecord,
    interned_text: &'a InternedText<'_>,
    ordinal: usize,
) -> Option<TextShapeInput<'a>> {
    match shape {
        ShapeRecord::Text {
            text,
            font,
            wrap,
            align,
            ..
        } => Some(TextShapeInput {
            ordinal: checked_text_ordinal(ordinal),
            text: text.resolve(interned_text),
            text_hash: text.hash,
            font: *font,
            wrap: *wrap,
            halign: align.halign(),
        }),
        _ => None,
    }
}

fn checked_text_ordinal(index: usize) -> u16 {
    u16::try_from(index).expect(
        "more than 65536 direct ShapeRecord::Text runs on one node; \
         widen the within-node ordinal width if this trips",
    )
}

/// Per-axis inputs for [`resolve_axis_size`]. Bundles the seven
/// numbers + `Sizing` into one struct so the call site reads as
/// "given this axis context, resolve the outer size" rather than a
/// 7-arg parameter cliff. `content_plus_padding` is the
/// margin-exclusive hug size (`content + padding`); `available` and
/// the returned value are margin-inclusive.
#[derive(Debug)]
pub(super) struct AxisCtx {
    pub(super) sizing: Sizing,
    pub(super) content_plus_padding: f32,
    pub(super) available: f32,
    pub(super) intrinsic_min: f32,
    pub(super) margin: f32,
    pub(super) min: f32,
    pub(super) max: f32,
}

/// **Contains-content rule:** Hug aims for content size, Fill aims
/// for `available`. Both floor at `max(content, intrinsic_min)` — a
/// node's rect always contains what's inside it. If the rigid floor
/// exceeds `available`, the node overflows its parent rather than its
/// content overflowing the node's rect. Downstream
/// (cascade/composer/backend) tolerates overflow, same as the
/// root-vs-surface case.
///
/// `content` here is the post-dispatch measured content size
/// (margin-exclusive). It already reflects wrapping/shrink under the
/// constrained available width, so on the cross axis of a wrapping
/// text leaf it's the correct multi-line height — unlike
/// `intrinsic_min`, which is computed pure-subtree at `available =
/// INFINITY` and only captures the single-line case. Hug needed both
/// (content already reflects wrapping; intrinsic_min catches rigid X
/// descendants like long unbreakable words). Fill needs both for the
/// same reason: `content` keeps the rect ≥ its measured content,
/// `intrinsic_min` keeps it ≥ rigid descendants the pure-subtree
/// query identified.
///
/// The two cases where desired exceeds `available`:
/// `max(content, intrinsic_min) > available` (rigid descendant or
/// post-wrap content doesn't fit) or `Sizing::fixed(v)`. An explicit
/// `min_size` floor applies on top of all three branches via the
/// trailing `clamp`.
///
/// `Fill` on an unconstrained axis (intrinsic queries with
/// `available = INFINITY`) collapses to its content size — matches
/// CSS Grid's `1fr` track in an auto-context parent.
pub(super) fn resolve_axis_size(ctx: AxisCtx) -> f32 {
    let rendered = if let Some(value) = ctx.sizing.fixed_value() {
        value
    } else if ctx.sizing.is_hug() {
        if ctx.available.is_finite() {
            ctx.content_plus_padding
                .min(ctx.available - ctx.margin)
                .max(ctx.intrinsic_min - ctx.margin)
        } else {
            ctx.content_plus_padding
        }
    } else {
        // WPF Stretch: Fill returns content at measure-time. The
        // "fill the slot" expansion happens at *arrange* — driver
        // arrange code redistributes leftover to Fill children
        // proportionally. Returning `available` here would balloon
        // any Hug ancestor to its grandparent's allocation (CSS auto-
        // sizing's classic Hug+Fill bug).
        ctx.content_plus_padding.max(ctx.intrinsic_min - ctx.margin)
    };
    rendered.max(0.0).clamp(ctx.min, ctx.max) + ctx.margin
}

/// Max over non-collapsed children's outer intrinsic on `axis`, each
/// child's contribution shifted by `offset`.
///
/// Drivers whose own size on an axis is "the largest child wants this
/// much" (ZStack, Stack cross-axis, WrapStack) pass
/// [`no_offset`] — Canvas is the one that folds in each child's declared
/// position. Same closure-parameter shape the measure side uses for the
/// identical split (`measure_per_axis_hug`, shared by `zstack::measure`
/// and `canvas::measure`).
pub(super) fn children_max_intrinsic(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    axis: Axis,
    query: IntrinsicQuery,
    interned_text: &InternedText<'_>,
    mut offset: impl FnMut(&Tree, NodeId) -> f32,
) -> IntrinsicRange {
    let mut range = IntrinsicRange::ZERO;
    for c in tree.active_children(node) {
        let child = query.child(layout, tree, c, axis, interned_text);
        let child_offset = offset(tree, c);
        for (req, slot) in range.requested(query) {
            *slot = slot.max(child.get(req) + child_offset);
        }
    }
    range
}

/// The [`children_max_intrinsic`] offset for drivers that place children
/// at the container origin — every one except Canvas.
#[inline]
pub(super) fn no_offset(_: &Tree, _: NodeId) -> f32 {
    0.0
}

/// Main-axis offset + effective inter-child gap for one row of
/// `justify`-distributed children. Single source of truth for Stack and
/// WrapStack — keeps SpaceBetween / SpaceAround degeneracy rules
/// (count < 2 / count < 1) in one place.
#[derive(Debug)]
pub(super) struct JustifyOffsets {
    pub(super) start: f32,
    pub(super) gap: f32,
}

pub(super) fn justify_offsets(
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
pub(super) fn measure_per_axis_hug(
    pass: &mut LayoutPass<'_>,
    node: NodeId,
    inner_avail: Size,
    mut contrib: impl FnMut(&Tree, NodeId, Size) -> Size,
) -> Size {
    let tree = pass.tree;
    let node_layout = tree.records.layout()[node.idx()];
    // Per-axis-hug availability: a `Hug` axis passes `INF` so the child
    // reports its natural size; a bounded axis passes the committed inner
    // extent. `INF` here is *height-given-width* via measure, not an
    // intrinsic-replaceable sentinel — replacing it with
    // `intrinsic(MaxContent)` looks equivalent for leaves but is wrong for
    // nested containers whose main-axis size depends on cross-axis (Grid
    // with wrapping cells, etc.): intrinsic queries the unbounded shape,
    // while INF-measure runs the child's full layout under the committed cross.
    let child_avail = Size::new(
        if node_layout.size.w().is_hug() {
            f32::INFINITY
        } else {
            inner_avail.w
        },
        if node_layout.size.h().is_hug() {
            f32::INFINITY
        } else {
            inner_avail.h
        },
    );
    let mut max_w = 0.0f32;
    let mut max_h = 0.0f32;
    for c in tree.active_children(node) {
        let d = pass.measure(c, child_avail);
        let cont = contrib(tree, c, d);
        max_w = max_w.max(cont.w);
        max_h = max_h.max(cont.h);
    }
    Size::new(max_w, max_h)
}

/// Per-axis alignment after the child→parent `Auto` fallback — what
/// [`resolved_axis_align`] hands back.
pub(super) struct AxisAlignPair {
    pub(super) h: AxisAlign,
    pub(super) v: AxisAlign,
}

/// Per-axis placement: chosen extent + offset within the parent's inner span.
#[derive(Debug)]
pub(super) struct AxisPlacement {
    pub(super) size: f32,
    pub(super) offset: f32,
}

#[inline]
pub(super) fn weighted_share(space: f32, weight: f32, total_weight: f64) -> f32 {
    (f64::from(space) * f64::from(weight) / total_weight) as f32
}

/// Resolve a child's alignment on both axes: child's own value if not `Auto`,
/// else the parent's `child_align` for that axis. Single source of truth for
/// the alignment cascade — every layout (stack, grid, zstack) calls this so
/// they can't drift. Stack discards the unused axis; the cost is two enum
/// matches per child per frame.
pub(super) fn resolved_axis_align(child: &LayoutCore, parent_child_align: Align) -> AxisAlignPair {
    let a = child.meta.align();
    AxisAlignPair {
        h: a.halign().or(parent_child_align.halign()).to_axis(),
        v: a.valign().or(parent_child_align.valign()).to_axis(),
    }
}

/// Outer size of a node arranged into `slot` on both axes with no
/// alignment — [`arrange_axis`] twice under [`AxisAlign::Auto`], keeping
/// only the extents.
///
/// The two callers that place a node without needing its alignment offset:
/// `LayoutEngine::run` sizing a layer root against the surface, and
/// `canvas::arrange` sizing an absolutely-positioned child against its
/// slot. Both position by other means (the root's `Placement`, the child's
/// declared `pos`), so the offset `arrange_axis` returns is dead to them.
/// Grid and ZStack call `arrange_axis` directly precisely because they do
/// use that offset.
pub(super) fn arrange_size(
    child: &LayoutCore,
    bounds: &BoundsExtras,
    desired: Size,
    slot: Size,
) -> Size {
    Size::new(
        arrange_axis(Axis::X, AxisAlign::Auto, child, bounds, desired, slot.w).size,
        arrange_axis(Axis::Y, AxisAlign::Auto, child, bounds, desired, slot.h).size,
    )
}

/// Resolve the outer extent and alignment offset for one arranged axis.
/// `Fixed` always keeps its measured extent. `Fill` and explicit `Stretch`
/// grow to their slot without shrinking below measured content, while the
/// node's outer min/max bounds remain authoritative.
pub(super) fn arrange_axis(
    axis: Axis,
    align: AxisAlign,
    child: &LayoutCore,
    bounds: &BoundsExtras,
    desired: Size,
    slot: f32,
) -> AxisPlacement {
    let margin = axis.spacing(child.margin);
    let min = axis.main(bounds.min_size) + margin;
    let max = axis.main(bounds.max_size) + margin;
    let desired = axis.main(desired).clamp(min, max);
    let sizing = axis.main_sizing(child.size);
    let stretch = sizing.fill_weight().is_some()
        || matches!(align, AxisAlign::Stretch) && sizing.fixed_value().is_none();
    let size = if stretch {
        slot.max(desired).clamp(min, max)
    } else {
        desired
    };
    let offset = match align {
        AxisAlign::Center => ((slot - size) * 0.5).max(0.0),
        AxisAlign::End => (slot - size).max(0.0),
        _ => 0.0,
    };
    AxisPlacement { size, offset }
}

/// Cross-axis placement for a child of a main-axis stack (Stack /
/// WrapStack). Resolves the alignment cascade, picks the cross axis
/// from the resolved (h, v) pair, and runs [`arrange_axis`] against the
/// child's cross sizing + desired + the parent's cross extent. Single
/// source of truth so the cascade rule can't drift between Stack and
/// WrapStack.
pub(super) fn cross_place(
    main_axis: Axis,
    child: &LayoutCore,
    bounds: &BoundsExtras,
    parent_child_align: Align,
    desired: Size,
    inner_cross: f32,
) -> AxisPlacement {
    let AxisAlignPair { h, v } = resolved_axis_align(child, parent_child_align);
    let cross_align = match main_axis {
        Axis::X => v,
        Axis::Y => h,
    };
    let cross_axis = match main_axis {
        Axis::X => Axis::Y,
        Axis::Y => Axis::X,
    };
    arrange_axis(cross_axis, cross_align, child, bounds, desired, inner_cross)
}

#[cfg(test)]
mod tests {
    use crate::common::hash;
    use crate::layout::support::{TextShapeInput, checked_text_ordinal};
    use crate::layout::types::align::HAlign;
    use crate::text::glyph_font::GlyphFont;
    use crate::text::key::TextShapeKey;
    use crate::text::wrap::TextWrap;
    use crate::text::{FontFamily, FontWeight};

    #[test]
    fn text_ordinal_covers_the_u16_domain_and_rejects_the_next_run() {
        assert_eq!(checked_text_ordinal(0), 0);
        assert_eq!(checked_text_ordinal(usize::from(u16::MAX)), u16::MAX);
        assert!(
            std::panic::catch_unwind(|| checked_text_ordinal(usize::from(u16::MAX) + 1)).is_err(),
            "the 65537th direct text run must exceed the identity key",
        );
    }

    #[test]
    fn shape_request_reuses_the_recorded_hash_and_rejects_stale_ones() {
        let input = |text_hash| TextShapeInput {
            ordinal: 0,
            text: "hello",
            text_hash,
            font: GlyphFont {
                size_px: 16.0,
                line_height_px: 19.2,
                family: FontFamily::Sans,
                weight: FontWeight::Regular,
            },
            wrap: TextWrap::SingleLine,
            halign: HAlign::Auto,
        };
        let request = input(hash::hash_str("hello")).shape_request();
        assert_eq!(request.text, "hello");
        assert_eq!(
            request.key,
            TextShapeKey::unbounded(
                hash::hash_str("hello"),
                GlyphFont {
                    size_px: 16.0,
                    line_height_px: 19.2,
                    family: FontFamily::Sans,
                    weight: FontWeight::Regular,
                },
            ),
            "the retained hash must mint the same key re-hashing would",
        );
        // A retained hash that no longer matches the resolved bytes is a
        // logic error the debug assert pins.
        let stale = std::panic::catch_unwind(|| input(1).shape_request());
        assert!(stale.is_err(), "stale retained hash must panic");
    }
}
