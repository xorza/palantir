//! A bar overlay's definition, and the thumb arithmetic shared by the layout driver and the widget's pointer mapping.

use crate::layout::drivers::scrollbars::bar_geometry::BarGeometry;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::scroll_axes::ScrollAxes;
use crate::primitives::math::domain;
use crate::primitives::math::float_hash::FloatHash;
use crate::primitives::math::num::F32Px;
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
    ///
    /// The track is the viewport's extent on `axis`: both the ratio the thumb
    /// expresses and, floored to whole logical pixels, the length it slides along.
    pub(crate) fn thumb(&self, axis: Axis, outer: Size, content: Size) -> Option<BarGeometry> {
        if !self.axes.pans(axis) {
            return None;
        }
        let track = axis.main(self.viewport(outer));
        let content = axis.main(content) * self.zoom;
        if track <= 0.0 || content <= track {
            return None;
        }
        // Quantized here, not at paint: the widget reads these back to map a drag or track click, so the grabbed bar must be the drawn bar. Whole logical pixels because physical snapping rounds a rect's min and max independently (`Rect::scaled_by`), so a thumb on fractional coordinates would change length as it travels; integer edges pin the length at integer DPR.
        // **One length, not two**: `travel` comes off the same floored length the thumb clamped into; flooring separately let a sub-pixel track take the 1 px minimum thumb and place it at -1.
        let floored = track.floor().max(1.0);
        let thumb_size = (track / content * track)
            .max(self.min_thumb)
            .fast_round()
            .clamp(1.0, floored);
        let travel = floored - thumb_size;
        let max_offset = content - track;
        let fraction = domain::share_of(axis.main_v(self.offset), max_offset).clamp(0.0, 1.0);
        Some(BarGeometry {
            track,
            thumb_size,
            thumb_offset: (fraction * travel).fast_round(),
            travel,
            max_offset,
        })
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
