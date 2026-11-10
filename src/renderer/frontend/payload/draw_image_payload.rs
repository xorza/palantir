//! One textured-quad draw.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::frontend::paint_sink::{PaintPayload, PaintSink};
use crate::renderer::frontend::payload::spin::Spin;
use crate::renderer::render_buffer::image_flags::ImageFlags;
use crate::shape::geometry::closed_geometry::PlacedClosed;
use glam::Vec2;

/// Image draw payload: `rect` is the logical-px paint rect (`fit` and intrinsic size folded in),
/// `uv_min`/`uv_size` crop the texture (non-trivial only for Cover), `tint` multiplies the texel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawImagePayload {
    pub(crate) rect: Rect,
    /// The shape that clips the image, a box or a triangle whose covering rect it fits in, or
    /// `None` for a plain image, which `rect` bounds alone.
    pub(crate) geometry: Option<PlacedClosed>,
    pub(crate) uv_min: Vec2,
    pub(crate) uv_size: Vec2,
    pub(crate) tint: RgbaF16,
    /// Registration id keying the backend's texture cache; `TextureId(0)` means no texture and skips the draw.
    pub(crate) handle: TextureId,
    /// Tile wrap, nearest sampling and minification tap mode, forwarded to
    /// [`ImageInstance::flags`](crate::renderer::render_buffer::image_draw_row::ImageInstance).
    /// [`ImageFlags::NONE`] takes one bilinear tap.
    pub(crate) flags: ImageFlags,
    /// A paint animation's turn.
    pub(crate) spin: Option<Spin>,
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

impl PaintPayload for DrawImagePayload {
    #[inline]
    fn faded(self, by: f32) -> Self {
        if by == 1.0 {
            return self;
        }
        Self {
            tint: self.tint.faded(by),
            ..self
        }
    }

    /// Paints nothing: zero-extent rect, transparent tint, or a null handle with no view.
    #[inline]
    fn is_noop(&self) -> bool {
        self.rect.is_paint_empty()
            || self.tint.is_noop()
            || (self.handle.0 == 0 && self.view.is_none())
    }

    fn paint(self, sink: &mut impl PaintSink) {
        sink.image(self);
    }
}
