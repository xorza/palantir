//! Composited image draw records.

#![expect(
    clippy::expl_impl_clone_on_copy,
    reason = "`soa_rs`'s `Soars` derive writes `Clone` by hand for the `Copy` rows it generates"
)]

use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::render_buffer::image_flags::ImageFlags;
use glam::Vec2;
use soa_rs::Soars;

/// One image draw row. SoA storage lets the backend upload `rows.instance()`
/// in one `write_buffer`; `id` is the registration id behind an `ImageHandle`.
#[derive(Soars, Clone, Copy, Debug, PartialEq)]
#[soa_derive(Debug)]
pub(crate) struct ImageDrawRow {
    pub(crate) id: TextureId,
    pub(crate) instance: ImageInstance,
}

/// Per-image GPU state. The shader samples at `uv_min + corner * uv_size` and
/// multiplies by `tint`; the UV pair carries the `ImageFit::Cover` crop.
#[padding_struct::padding_struct]
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ImageInstance {
    /// Physical-px paint rect.
    pub(crate) rect: Rect,
    /// UV crop top-left (0..1 texture coords).
    pub(crate) uv_min: Vec2,
    /// UV crop extent: `(1, 1)` normally, smaller for `Cover`, `> 1` for `Tile`.
    pub(crate) uv_size: Vec2,
    /// Linear-RGBA tint, premultiplied in the shader.
    pub(crate) tint: RgbaF16,
    /// Tile wrap, min/mag nearest sampling and minification tap mode.
    pub(crate) flags: ImageFlags,
}
