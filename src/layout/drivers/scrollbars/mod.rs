//! Layout-side scrollbar driver: places the four bar leaves from geometry that exists only after measure, since a thumb's extent is a content/viewport ratio unknown while recording ([`LayerLayout::scroll_content`](crate::layout::layer_layout::LayerLayout::scroll_content)).
//!
//! Children are recorded in a fixed order (vertical track, vertical thumb, horizontal track, horizontal thumb) and an absent bar arranges zero-extent, so the child list and bar ids stay stable across an overflow toggle.

pub(crate) mod bar_geometry;
pub(crate) mod scrollbars_def;

use crate::layout::drivers::LayoutDriver;
use crate::layout::intrinsic::intrinsic_pass::IntrinsicPass;
use crate::layout::intrinsic::intrinsic_query::IntrinsicQuery;
use crate::layout::intrinsic::intrinsic_range::IntrinsicRange;
use crate::layout::measured::Measured;
use crate::layout::pass::LayoutPass;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::axis::{Axis, AxisExtent};
use crate::primitives::layout::layout_mode::ScrollbarsDefId;
use crate::scene::tree::node_id::NodeId;
use glam::Vec2;

/// One axis' track and thumb rects in overlay-local coordinates.
#[derive(Copy, Clone, Debug, PartialEq)]
struct BarRects {
    track: Rect,
    thumb: Rect,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Scrollbars {
    node: NodeId,
    /// Index of this overlay's definition in `Tree::scrollbar_defs`.
    def: ScrollbarsDefId,
}

impl Scrollbars {
    pub(super) const fn new(node: NodeId, def: ScrollbarsDefId) -> Self {
        Self { node, def }
    }
}

impl LayoutDriver for Scrollbars {
    /// Thumbs size from the sibling viewport's measured `scroll_content`, so content that stops overflowing leaves this subtree's hash and slot untouched.
    const ARRANGE_DEPENDS_ONLY_ON_SLOT: bool = false;

    /// Bars never inflate their scroll: the overlay reports `ZERO`. The four leaves are still measured so their `desired` rows are written; `capture_tree` stores the layer's whole column and an unwritten row would replay stale scratch.
    fn measure(self, pass: &mut LayoutPass<'_>, inner_avail: Size) -> Measured {
        // `tree` reborrows independently of `pass`, so the walk and the recursion coexist.
        let tree = pass.tree;
        let mut stable_from = Size::ZERO;
        for child in tree.children(self.node) {
            stable_from = stable_from.max(pass.measure(child.id, inner_avail).stable_from);
        }
        Measured {
            stable_from,
            ..Measured::ZERO
        }
    }

    /// Give each bar leaf its resolved rect, zero-extent for an absent axis; child order is the recording contract above. `inner` is the overlay's arranged size (Fill on both axes).
    fn arrange(self, pass: &mut LayoutPass<'_>, inner: Size) {
        let resolved = pass.tree.scrollbar_defs[usize::from(self.def)];
        let def = resolved.def;
        let content = pass.scroll_content(resolved.content);
        let bar_rects = |axis: Axis| {
            let bar = def.thumb(axis, inner, content)?;
            // The bar sits in the far-edge strip of the outer extent that `reserve` set aside.
            let cross = AxisExtent::new(axis.cross(inner) - def.bar_thickness, def.bar_thickness);
            Some(BarRects {
                track: axis.compose_rect(AxisExtent::new(0.0, bar.track), cross),
                thumb: axis.compose_rect(AxisExtent::new(bar.thumb_offset, bar.thumb_size), cross),
            })
        };
        let vertical = bar_rects(Axis::Y);
        let horizontal = bar_rects(Axis::X);

        let slots = [
            vertical.map(|b| b.track),
            vertical.map(|b| b.thumb),
            horizontal.map(|b| b.track),
            horizontal.map(|b| b.thumb),
        ];
        let tree = pass.tree;
        for (child, slot) in tree.children(self.node).zip(slots) {
            // An absent bar collapses to zero extent at the overlay origin, so its `WidgetId` keeps its state row and the child list keeps its shape.
            let rect = slot.unwrap_or(Rect {
                min: Vec2::ZERO,
                size: Size::ZERO,
            });
            pass.arrange(child.id, rect);
        }
    }

    /// Bars are absolutely placed chrome in a reserved gutter and never floor their scroll; the driver contributes nothing to a parent's intrinsic, so every argument goes unread.
    fn intrinsic(self, _pass: &mut IntrinsicPass<'_>, _query: IntrinsicQuery) -> IntrinsicRange {
        IntrinsicRange::ZERO
    }
}
