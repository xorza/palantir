//! Layout-side scrollbar driver: places the four bar leaves (vertical
//! track + thumb, horizontal track + thumb) from geometry that does not
//! exist until measure has run.
//!
//! A scroll widget cannot do this while recording. A thumb's extent is a
//! content/viewport *ratio*, and on a scroll's first frame neither term is
//! available: the viewport comes from the previous pass's arranged rect,
//! and the content extent is written by the viewport's own measure into
//! [`LayerLayout::scroll_content`](crate::layout::LayerLayout::scroll_content).
//! Resolving it here is what lets the widget record its bars
//! unconditionally instead of asking `Ui` to re-record the whole frame.
//!
//! Children are recorded in a fixed order (vertical track, vertical
//! thumb, horizontal track, horizontal thumb) and an *absent* bar
//! arranges zero-extent rather than going unrecorded, so the child list
//! keeps the same shape every frame and the bar ids stay stable across
//! an overflow toggle.

pub(crate) mod scrollbars_def;

use crate::layout::axis::Axis;
use crate::layout::driver::LayoutDriver;
use crate::layout::engine::LayoutEngine;
use crate::layout::intrinsic::{IntrinsicQuery, IntrinsicRange};
use crate::layout::pass::LayoutPass;
use crate::layout::scrollbars::scrollbars_def::ScrollbarsDef;
use crate::layout::types::layout_mode::ScrollbarsDefId;
use crate::primitives::interned_text::InternedText;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::scene::tree::Tree;
use crate::scene::tree::node_id::NodeId;
use glam::Vec2;

/// One axis' track and thumb rects in overlay-local coordinates, or
/// `None` when that axis shows no bar.
#[derive(Copy, Clone, Debug, PartialEq)]
struct BarRects {
    track: Rect,
    thumb: Rect,
}

/// Resolve one axis. `outer` is the overlay's arranged size, which is
/// the scroll's outer rect — the overlay is `Fill` on both axes.
fn axis_rects(def: &ScrollbarsDef, outer: Size, content: Size, axis: Axis) -> Option<BarRects> {
    let bar = def.thumb(axis, outer, content)?;
    // The bar sits in the far-edge strip of the *outer* extent, not the
    // viewport's: that strip is exactly what `reserve` set aside.
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

    /// Sizes its thumbs from the *sibling* viewport's measured
    /// `scroll_content`, so content that stops overflowing leaves this
    /// subtree's own hash and slot untouched while the bars it should
    /// retire stay exactly where they were.
    const ARRANGE_DEPENDS_ONLY_ON_SLOT: bool = false;

    /// Bars never inflate their scroll: the overlay reports `ZERO` so a
    /// `Hug` ancestor sizes to content alone.
    ///
    /// The four leaves are still measured, although `arrange` below
    /// places every one of them by rect and reads none of the answers.
    /// Filling their `desired` rows is the point: `capture_tree` stores
    /// the layer's whole column and a measure hit restores a whole
    /// subtree's slice of it, so a row this driver left unwritten would
    /// carry whatever the retained scratch held at that index last
    /// frame, and the cache would store and replay that.
    fn measure(
        pass: &mut LayoutPass<'_>,
        node: NodeId,
        _id: Self::Payload,
        inner_avail: Size,
    ) -> Size {
        // `tree` is a shared reborrow independent of `pass`, so the child
        // walk and the `&mut pass` recursion coexist without buffering.
        let tree = pass.tree;
        for child in tree.children(node) {
            pass.measure(child.id, inner_avail);
        }
        Size::ZERO
    }

    /// Assign each of the four bar leaves its resolved rect, zero-extent for
    /// an axis that shows no bar. Child order is the recording contract from
    /// this module's doc.
    fn arrange(pass: &mut LayoutPass<'_>, node: NodeId, id: Self::Payload, inner: Rect) {
        let resolved = pass.tree.scrollbar_defs[usize::from(id)];
        let content = pass.scroll_content(resolved.content);
        let vertical = axis_rects(&resolved.def, inner.size, content, Axis::Y);
        let horizontal = axis_rects(&resolved.def, inner.size, content, Axis::X);

        let slots = [
            vertical.map(|b| b.track),
            vertical.map(|b| b.thumb),
            horizontal.map(|b| b.track),
            horizontal.map(|b| b.thumb),
        ];
        let tree = pass.tree;
        for (child, slot) in tree.children(node).zip(slots) {
            // An absent bar collapses to zero extent at the overlay origin
            // rather than going unrecorded, so its `WidgetId` keeps its state
            // row and the child list stays the same shape whether or not
            // content currently overflows. Zero extent paints nothing and
            // cannot be hit, so the origin is as good a place as any.
            let local = slot.unwrap_or(Rect {
                min: Vec2::ZERO,
                size: Size::ZERO,
            });
            let rect = Rect {
                min: inner.min + local.min,
                size: local.size,
            };
            pass.arrange(child.id, rect);
        }
    }

    /// Bars are absolutely placed chrome in a reserved gutter: they must never
    /// floor the scroll they decorate, on either axis. The whole driver
    /// contributes nothing to a parent's intrinsic, so every argument goes
    /// unread — the answer is still the driver's to give rather than a `ZERO`
    /// written into the dispatch.
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
