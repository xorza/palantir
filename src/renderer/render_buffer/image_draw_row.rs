//! Composited image draw records.

#![expect(
    clippy::expl_impl_clone_on_copy,
    reason = "`soa_rs`'s `Soars` derive writes `Clone` by hand for the `Copy` rows it generates"
)]

use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::render_buffer::image_flags::ImageFlags;
use crate::renderer::render_buffer::shape_form::ShapeForm;
use crate::renderer::render_buffer::shape_header::ShapeHeader;
use glam::Vec2;
use soa_rs::Soars;

/// One image draw row. SoA storage lets the backend upload `rows.instance()`
/// in one `write_buffer`; `id` is the registration id behind an `ImageHandle`.
#[derive(Soars, Clone, Copy, Debug, PartialEq)]
#[soa_derive(Debug)]
pub(crate) struct ImageDrawRow {
    pub(crate) id: TextureId,
    /// The geometry the draw takes, which picks its pipeline.
    pub(crate) key: ShapeForm,
    pub(crate) instance: ImageInstance,
}

/// Per-image GPU state. The shader samples at `uv_min + corner * uv_size` and multiplies by
/// `tint`; the UV pair carries the `Fit::Cover` crop.
#[padding_struct::padding_struct]
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ImageInstance {
    /// The quad the image draws over, physical px, its turn and its rounded clips; and for an
    /// image of a box's or a triangle's [`ShapeForm`], the shape that clips it, which a plain
    /// image leaves unread.
    pub(crate) header: ShapeHeader,
    /// UV crop top-left (0..1 texture coords).
    pub(crate) uv_min: Vec2,
    /// UV crop extent: `(1, 1)` normally, smaller for `Cover`, `> 1` for `Tile`.
    pub(crate) uv_size: Vec2,
    /// Linear-RGBA tint, premultiplied in the shader.
    pub(crate) tint: RgbaF16,
    /// Tile wrap, min/mag nearest sampling and minification tap mode.
    pub(crate) flags: ImageFlags,
    /// The image's own rect relative to `header.rect.min`, past which a shaped image shows
    /// nothing. An edge on the shape's, within the composer's slack, or past it is out of reach
    /// (`f32::MAX`), so it fades with the shape's.
    pub(crate) decal_min: Vec2,
    pub(crate) decal_max: Vec2,
}
