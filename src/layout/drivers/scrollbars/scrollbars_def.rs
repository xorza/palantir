//! A bar overlay's definition, and the thumb arithmetic both the layout
//! driver that places the bars and the widget that maps pointer input
//! onto them read.

use crate::layout::drivers::scrollbars::bar_geometry::BarGeometry;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::scroll_axes::ScrollAxes;
use crate::primitives::math::float_hash::FloatHash;
use crate::scene::tree::node_id::NodeId;
use glam::Vec2;
use std::hash::{Hash, Hasher};

/// What a scrollbars overlay (the crate-internal `Widget::scrollbars`)
/// places its bars from.
///
/// Everything here is known while recording except the viewport's content
/// extent, which exists only once measure has run. So the overlay names the
/// viewport, and its layout reads the extent then. That is what lets a
/// scroll widget record its bars on its first frame, with no second pass.
///
/// Installed with the crate-internal `Widget::scrollbar_def`. A widget
/// reads the same numbers through [`Self::thumb`] to map a thumb
/// drag or a track click onto an offset, so the bar the user grabs is the
/// bar that was drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ScrollbarsDef {
    /// The [`Widget::scroll`](crate::widget::Widget::scroll) viewport whose
    /// content the bars show. It must be recorded earlier in the same
    /// frame, on the overlay's layer.
    pub(crate) content: WidgetId,
    /// How far the content is scrolled, in pixels of the zoomed content:
    /// the translation the viewport's transform carries, negated.
    pub(crate) offset: Vec2,
    /// The content's scale. The bars show the content at this size.
    pub(crate) zoom: f32,
    /// The viewport's axes. An axis that does not pan shows no bar.
    pub(crate) axes: ScrollAxes,
    /// The gutter the bars take out of the overlay's box, per side. Zero
    /// for bars that paint over the content.
    ///
    /// Keep it independent of whether the content overflows: a gutter
    /// that opens with the bar makes a `Hug` ancestor jump when it does.
    pub(crate) reserve: Spacing,
    /// The viewport's own padding, inside the gutter.
    pub(crate) padding: Spacing,
    /// A bar's breadth across its axis. The vertical bar runs along the
    /// overlay's right edge, and the horizontal bar along its bottom edge.
    pub(crate) bar_thickness: f32,
    /// The shortest a thumb gets, however long the content.
    pub(crate) min_thumb: f32,
}

impl ScrollbarsDef {
    /// The strip the content occupies in an overlay of size `outer`:
    /// `outer` less the gutter and the padding, floored at zero.
    ///
    /// The layout driver passes this frame's arranged size, to place the
    /// bars. A widget passes the size it arranged at last frame, to solve
    /// its offset in.
    pub(crate) fn viewport(&self, outer: Size) -> Size {
        Size::new(
            (outer.w - self.reserve.horizontal_sum() - self.padding.horizontal_sum()).max(0.0),
            (outer.h - self.reserve.vertical_sum() - self.padding.vertical_sum()).max(0.0),
        )
    }

    /// The bar on `axis` in an overlay of size `outer`, over content that
    /// measured `content` before zoom — the extent
    /// [`Ui::scroll_content`](crate::Ui::scroll_content) reports.
    ///
    /// `None` when the axis does not pan, when the viewport is empty, or
    /// when the content fits and no thumb shows.
    pub(crate) fn thumb(&self, axis: Axis, outer: Size, content: Size) -> Option<BarGeometry> {
        if !self.axes.pans(axis) {
            return None;
        }
        BarGeometry::along(
            axis.main(self.viewport(outer)),
            axis.main(content) * self.zoom,
            axis.main_v(self.offset),
            self.min_thumb,
        )
    }

    /// Visual hash for the authoring rollup. The fit flags stay out: they
    /// size the viewport, and the bars read only which axes pan.
    pub(crate) fn hash_visual<H: Hasher>(&self, h: &mut H) {
        self.content.hash(h);
        self.offset.hash_visual(h);
        self.zoom.hash_visual(h);
        h.write_u16(self.axes.pan_bits());
        self.reserve.hash(h);
        self.padding.hash(h);
        self.bar_thickness.hash_visual(h);
        self.min_thumb.hash_visual(h);
    }
}

/// A [`ScrollbarsDef`] as the tree stores it, with the viewport it names
/// resolved to the node this pass recorded it as.
///
/// Valid for exactly the pass that resolved it: the tree is rebuilt every
/// pass, and every pass records the def again.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ResolvedScrollbarsDef {
    pub(crate) def: ScrollbarsDef,
    pub(crate) content: NodeId,
}
