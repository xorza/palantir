//! Layout-side scrollbar driver: places the four bar leaves from geometry that exists only after measure, since a thumb's extent is a content/viewport ratio unknown while recording ([`LayerLayout::scroll_content`](crate::layout::layer_layout::LayerLayout::scroll_content)).
//!
//! Children are recorded in a fixed order (vertical track, vertical thumb, horizontal track, horizontal thumb) and an absent bar arranges zero-extent, so the child list and bar ids stay stable across an overflow toggle.

pub(crate) mod bar_geometry;
pub(crate) mod scrollbars_def;

use crate::layout::drivers::LayoutDriver;
use crate::layout::drivers::scrollbars::scrollbars_def::ScrollbarsDef;
use crate::layout::engine::LayoutEngine;
use crate::layout::intrinsic::intrinsic_query::IntrinsicQuery;
use crate::layout::intrinsic::intrinsic_range::IntrinsicRange;
use crate::layout::measured::Measured;
use crate::layout::pass::LayoutPass;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::layout_mode::ScrollbarsDefId;
use crate::primitives::text::interned_text::InternedText;
use crate::scene::tree::Tree;
use crate::scene::tree::node_id::NodeId;
use glam::Vec2;

/// One axis' track and thumb rects in overlay-local coordinates, or `None` when no bar.
#[derive(Copy, Clone, Debug, PartialEq)]
struct BarRects {
    track: Rect,
    thumb: Rect,
}

/// Resolve one axis; `outer` is the overlay's arranged size (Fill on both axes).
fn axis_rects(def: &ScrollbarsDef, outer: Size, content: Size, axis: Axis) -> Option<BarRects> {
    let bar = def.thumb(axis, outer, content)?;
    // The bar sits in the far-edge strip of the outer extent that `reserve` set aside.
    let cross_pos = axis.cross(outer) - def.bar_thickness;
    Some(BarRects {
        track: axis.compose_rect(0.0, cross_pos, bar.track, def.bar_thickness),
        thumb: axis.compose_rect(
            bar.thumb_offset,
            cross_pos,
            bar.thumb_size,
            def.bar_thickness,
        ),
    })
}

#[derive(Debug)]
pub(super) struct Scrollbars;

impl LayoutDriver for Scrollbars {
    /// Index of this overlay's definition in `Tree::scrollbar_defs`.
    type Payload = ScrollbarsDefId;

    /// Thumbs size from the sibling viewport's measured `scroll_content`, so content that stops overflowing leaves this subtree's hash and slot untouched.
    const ARRANGE_DEPENDS_ONLY_ON_SLOT: bool = false;

    /// Bars never inflate their scroll: the overlay reports `ZERO`. The four leaves are still measured so their `desired` rows are written; `capture_tree` stores the layer's whole column and an unwritten row would replay stale scratch.
    fn measure(
        pass: &mut LayoutPass<'_>,
        node: NodeId,
        _id: Self::Payload,
        inner_avail: Size,
    ) -> Measured {
        // `tree` reborrows independently of `pass`, so the walk and the recursion coexist.
        let tree = pass.tree;
        let mut stable_from = Size::ZERO;
        for child in tree.children(node) {
            stable_from = stable_from.max(pass.measure(child.id, inner_avail).stable_from);
        }
        Measured {
            stable_from,
            ..Measured::ZERO
        }
    }

    /// Give each bar leaf its resolved rect, zero-extent for an absent axis; child order is the recording contract above.
    fn arrange(pass: &mut LayoutPass<'_>, node: NodeId, id: Self::Payload, inner: Size) {
        let resolved = pass.tree.scrollbar_defs[usize::from(id)];
        let content = pass.scroll_content(resolved.content);
        let vertical = axis_rects(&resolved.def, inner, content, Axis::Y);
        let horizontal = axis_rects(&resolved.def, inner, content, Axis::X);

        let slots = [
            vertical.map(|b| b.track),
            vertical.map(|b| b.thumb),
            horizontal.map(|b| b.track),
            horizontal.map(|b| b.thumb),
        ];
        let tree = pass.tree;
        for (child, slot) in tree.children(node).zip(slots) {
            // An absent bar collapses to zero extent at the overlay origin, so its `WidgetId` keeps its state row and the child list keeps its shape.
            let rect = slot.unwrap_or(Rect {
                min: Vec2::ZERO,
                size: Size::ZERO,
            });
            pass.arrange(child.id, rect);
        }
    }

    /// Bars are absolutely placed chrome in a reserved gutter and never floor their scroll; the driver contributes nothing to a parent's intrinsic, so every argument goes unread.
    fn intrinsic(
        _engine: &mut LayoutEngine,
        _tree: &Tree,
        _node: NodeId,
        _id: Self::Payload,
        _axis: Axis,
        _query: IntrinsicQuery,
        _interned_text: &InternedText<'_>,
    ) -> IntrinsicRange {
        IntrinsicRange::ZERO
    }
}
