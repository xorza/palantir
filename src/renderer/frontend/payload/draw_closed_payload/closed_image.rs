//! [`ClosedImage`]: an image over a closed shape.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::draw_closed_payload::{
    ClosedPaintPayload, DrawClosedPayload,
};
use crate::renderer::render_buffer::image_flags::ImageFlags;
use crate::shape::geometry::closed_geometry::PlacedClosed;
use glam::Vec2;

/// An image over a closed shape: `rect` is where the image lies, logical px, its fit and
/// intrinsic size folded in, past which it shows nothing, as CSS's `background-repeat:
/// no-repeat` lays an image in its box; `uv_min`/`uv_size` crop the texture (non-trivial only for
/// Cover), `tint` multiplies the texel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ClosedImage {
    pub(crate) rect: Rect,
    pub(crate) uv_min: Vec2,
    pub(crate) uv_size: Vec2,
    pub(crate) tint: RgbaF16,
    /// Registration id keying the backend's texture cache; `TextureId(0)` means no texture and skips the draw.
    pub(crate) handle: TextureId,
    /// Tile wrap, nearest sampling and minification tap mode, forwarded to
    /// [`ImageInstance::flags`](crate::renderer::render_buffer::image_draw_row::ImageInstance).
    /// [`ImageFlags::NONE`] takes one bilinear tap.
    pub(crate) flags: ImageFlags,
    /// The `GpuView` whose target `handle` names, which the frame paints before it composites.
    pub(crate) view: Option<GpuViewDraw>,
}

/// The `GpuView` an image draw composites: the widget whose callback paints its target, and the
/// view's repaint version, under which a target painted at this geometry is still valid. The
/// callback stays in the window's `GpuViews`, which the backend reads by `owner`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GpuViewDraw {
    pub(crate) owner: WidgetId,
    pub(crate) epoch: u64,
}

impl ClosedPaintPayload for ClosedImage {
    #[inline]
    fn faded(self, by: f32) -> Self {
        Self {
            tint: self.tint.faded(by),
            ..self
        }
    }

    /// Paints nothing: an image rect of no extent, a transparent tint, or a null handle with no
    /// view. The shape is not read: a `Natural` image in an owner of no size still lies at its
    /// own size.
    #[inline]
    fn is_noop(&self, _geometry: PlacedClosed) -> bool {
        self.rect.is_paint_empty()
            || self.tint.is_noop()
            || (self.handle.0 == 0 && self.view.is_none())
    }

    fn paint(draw: DrawClosedPayload<Self>, sink: &mut impl PaintSink) {
        sink.image(draw);
    }
}
