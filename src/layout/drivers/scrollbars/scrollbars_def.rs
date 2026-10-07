//! A bar overlay's definition, and the thumb arithmetic shared by the layout driver and the widget's pointer mapping.

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

/// What a scrollbars overlay places its bars from. The viewport's content extent exists only after measure, so the overlay names the viewport and layout reads the extent then; a widget reads [`Self::thumb`] to map drags and clicks, so the grabbed bar is the drawn bar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ScrollbarsDef {
    /// The [`Widget::scroll`](crate::widget::Widget::scroll) viewport whose content the bars show; recorded earlier in the frame.
    pub(crate) content: WidgetId,
    pub(crate) offset: Vec2,
    pub(crate) zoom: f32,
    pub(crate) axes: ScrollAxes,
    /// Gutter the bars take out of the overlay's box, per side; keep it independent of overflow, or a `Hug` ancestor jumps.
    pub(crate) reserve: Spacing,
    pub(crate) padding: Spacing,
    pub(crate) bar_thickness: f32,
    /// The shortest a thumb gets, however long the content.
    pub(crate) min_thumb: f32,
}

impl ScrollbarsDef {
    /// The strip the content occupies in an overlay of size `outer`: `outer` less gutter and padding, floored at zero.
    pub(crate) fn viewport(&self, outer: Size) -> Size {
        let (reserve, padding) = (self.reserve.sums(), self.padding.sums());
        Size::new(
            (outer.w - reserve.w - padding.w).max(0.0),
            (outer.h - reserve.h - padding.h).max(0.0),
        )
    }

    /// The bar on `axis` over content that measured `content` before zoom ([`Ui::scroll_content`](crate::Ui::scroll_content)); `None` when the axis does not pan, the viewport is empty, or the content fits.
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

    /// Visual hash for the authoring rollup; fit flags stay out (they size the viewport).
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

/// A [`ScrollbarsDef`] as the tree stores it, with the viewport resolved to the node; valid for the pass that resolved it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ResolvedScrollbarsDef {
    pub(crate) def: ScrollbarsDef,
    pub(crate) content: NodeId,
}
