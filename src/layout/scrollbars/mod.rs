//! Layout-side scrollbar driver: places the four bar leaves (vertical
//! track + thumb, horizontal track + thumb) from geometry that does not
//! exist until measure has run.
//!
//! `Scroll` cannot do this while recording. A thumb's extent is a
//! content/viewport *ratio*, and on a scroll's first frame neither term is
//! available: the viewport comes from the previous pass's arranged rect,
//! and the content extent is written by `scroll::measure` into
//! [`LayerLayout::scroll_content`](crate::layout::LayerLayout::scroll_content).
//! Resolving it here is what lets `Scroll` record its bars unconditionally
//! instead of asking `Ui` to re-record the whole frame.
//!
//! Children are recorded in a fixed order (vertical track, vertical
//! thumb, horizontal track, horizontal thumb) and an *absent* bar
//! arranges zero-extent rather than going unrecorded, so the child list
//! keeps the same shape every frame and the bar ids stay stable across
//! an overflow toggle.

use glam::{BVec2, Vec2};
use std::hash::Hash;

use crate::layout::axis::Axis;
use crate::layout::pass::LayoutPass;
use crate::primitives::approx;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::scene::tree::record::NodeId;

/// The strip a scroll's content actually occupies: its own extent less
/// the reserved bar gutters and the user padding.
///
/// The single formula, called with two different `outer`s: the driver
/// passes this frame's arranged overlay size to place the bars, while
/// `Scroll` passes the previous pass's outer size to convert a thumb
/// drag into an offset. They have to agree, or the bar the user grabs
/// and the bar that gets drawn scale differently.
pub(crate) fn viewport(outer: Size, reserve_y: f32, reserve_x: f32, padding: Spacing) -> Size {
    Size::new(
        (outer.w - reserve_y - padding.horiz()).max(0.0),
        (outer.h - reserve_x - padding.vert()).max(0.0),
    )
}

/// Everything the driver needs that *is* known while recording, plus a
/// handle to the viewport whose measured content it isn't.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ScrollbarsDef {
    /// Viewport node whose
    /// [`LayerLayout::scroll_content`](crate::layout::LayerLayout::scroll_content)
    /// sizes the thumbs. Resolved at record time out of the *current*
    /// pass's id map, so it is valid for exactly the pass that recorded it
    /// — the tree is rebuilt every pass and every pass re-records this def.
    pub(crate) content: NodeId,
    pub(crate) offset: Vec2,
    pub(crate) zoom: f32,
    pub(crate) pan: BVec2,
    /// Cross-axis gutter strips deducted from the viewport, and the user
    /// padding inside it. Both are content-independent — `bar_space`
    /// reserves on `pan && Reserved` alone, never on overflow — so they
    /// are known while recording even when nothing else is.
    pub(crate) reserve_y: f32,
    pub(crate) reserve_x: f32,
    pub(crate) padding: Spacing,
    pub(crate) bar_width: f32,
    pub(crate) min_thumb: f32,
}

impl ScrollbarsDef {
    /// Visual hash for the authoring rollup. `content` is deliberately
    /// **excluded**: it is a positional index that shifts whenever any
    /// earlier sibling's subtree grows, which would invalidate this
    /// scroll's measure cache for a change that cannot affect its bars.
    /// The owning node's `WidgetId` is folded separately and is what
    /// actually distinguishes two scrolls.
    pub(crate) fn hash_visual<H: std::hash::Hasher>(&self, h: &mut H) {
        approx::hash_visual_vec2(self.offset, h);
        approx::hash_visual_f32(self.zoom, h);
        h.write_u8(u8::from(self.pan.x) | (u8::from(self.pan.y) << 1));
        approx::hash_visual_f32(self.reserve_y, h);
        approx::hash_visual_f32(self.reserve_x, h);
        self.padding.hash(h);
        approx::hash_visual_f32(self.bar_width, h);
        approx::hash_visual_f32(self.min_thumb, h);
    }
}

/// Thumb extent and travel along one axis, or `None` when the bar can't
/// be drawn meaningfully — a non-positive viewport, or content that fits.
/// The "content fits" arm is what makes an idle scroll show no thumb.
///
/// Both fields are already quantized to whole logical pixels, so the
/// driver paints them as they arrive and `Scroll` maps pointer input
/// against the same numbers that were drawn.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct BarGeometry {
    pub(crate) thumb_size: f32,
    pub(crate) thumb_offset: f32,
}

/// A bar's track always spans its axis' whole viewport extent, so
/// `viewport` is both the ratio the thumb expresses and the length it
/// travels along.
pub(crate) fn bar_geometry(
    viewport: f32,
    content: f32,
    offset: f32,
    min_thumb: f32,
) -> Option<BarGeometry> {
    if viewport <= 0.0 || content <= viewport {
        return None;
    }
    let raw = viewport / content * viewport;
    let thumb_size = raw.max(min_thumb).min(viewport);
    let max_off = (content - viewport).max(f32::EPSILON);
    let travel = (viewport - thumb_size).max(0.0);
    let thumb_offset = (offset / max_off).clamp(0.0, 1.0) * travel;
    // Quantize here rather than at the paint site: the widget reads
    // this to map a drag or a track click back onto an offset, so the
    // bar the user grabs has to be the bar that was drawn. Rounding on
    // one side only put drag scaling up to a pixel out and made a click
    // in the rounding sliver page away from the thumb.
    //
    // Whole logical pixels because physical snapping rounds a rect's
    // min and max *independently* (`Rect::scaled_by`), which keeps
    // adjacent rects flush but makes a rect's snapped *length* depend on
    // where it sits — so a thumb on fractional coordinates visibly grows
    // and shrinks by a pixel as it travels. Integer logical edges scale
    // to integer physical ones at integer DPR, which pins the length; at
    // fractional DPR it only narrows the wobble, since the real cause is
    // in the snap.
    let thumb_size = thumb_size.round().min(viewport.floor()).max(1.0);
    let thumb_offset = thumb_offset.round().min(viewport.floor() - thumb_size);
    Some(BarGeometry {
        thumb_size,
        thumb_offset,
    })
}

/// One axis' track and thumb rects in overlay-local coordinates, or
/// `None` when that axis shows no bar.
#[derive(Copy, Clone, Debug, PartialEq)]
struct BarRects {
    track: Rect,
    thumb: Rect,
}

/// Resolve one axis. `outer` is the overlay's arranged size, which is
/// the scroll's outer rect — the overlay is `Fill` on both axes.
fn axis_rects(
    def: &ScrollbarsDef,
    outer: Size,
    scaled_content: Size,
    axis: Axis,
) -> Option<BarRects> {
    let panned = match axis {
        Axis::X => def.pan.x,
        Axis::Y => def.pan.y,
    };
    if !panned {
        return None;
    }
    let viewport = viewport(outer, def.reserve_y, def.reserve_x, def.padding);
    let main = axis.main(viewport);
    let geom = bar_geometry(
        main,
        axis.main(scaled_content),
        axis.main_v(def.offset),
        def.min_thumb,
    )?;
    // The bar sits in the far-edge strip of the *outer* extent, not the
    // viewport's: that strip is exactly what `reserve_*` set aside.
    let cross_pos = axis.cross(outer) - def.bar_width;
    Some(BarRects {
        track: axis.compose_rect(0.0, cross_pos, main, def.bar_width),
        thumb: axis.compose_rect(geom.thumb_offset, cross_pos, geom.thumb_size, def.bar_width),
    })
}

/// Bars never inflate their scroll: the overlay reports `ZERO` so a
/// `Hug` ancestor sizes to content alone. Children are still measured so
/// their `desired` rows exist for the arrange below.
pub(super) fn measure(pass: &mut LayoutPass<'_>, node: NodeId, inner_avail: Size) -> Size {
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
pub(super) fn arrange(pass: &mut LayoutPass<'_>, node: NodeId, inner: Rect, def: ScrollbarsDef) {
    let raw_content = pass.scroll_content(def.content);
    let scaled_content = Size::new(raw_content.w * def.zoom, raw_content.h * def.zoom);
    let vertical = axis_rects(&def, inner.size, scaled_content, Axis::Y);
    let horizontal = axis_rects(&def, inner.size, scaled_content, Axis::X);

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
