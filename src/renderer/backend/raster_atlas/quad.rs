//! The draw program a [`RasterAtlas`](crate::renderer::backend::raster_atlas::RasterAtlas)
//! is read through: one instance type, one shader, one bind-group shape.
//!
//! Both tenants draw the same rectangle — a tinted quad sampling one atlas
//! slot — so the instance, the shader, and the group-0 layout live here with
//! the atlas rather than inside whichever pass happened to need them first.
//! What differs between the passes is only which atlas they bind and where
//! their pixels came from.

use crate::renderer::backend::pipeline_utils;
use crate::renderer::backend::raster_atlas::ContentType;
use crate::renderer::backend::viewport::ViewportPush;

/// One per-instance vertex record. 20 bytes, `Pod`.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct RasterQuad {
    /// Top-left in physical px.
    pub(crate) pos: [i32; 2],
    /// Extents, packed by [`Self::dim`].
    pub(crate) dim: u32,
    /// Atlas origin plus content type, packed by [`pack_uv`].
    pub(crate) uv_and_kind: u32,
    /// Straight-alpha linear RGBA; the shader premultiplies at output.
    pub(crate) color: u32,
}

impl RasterQuad {
    /// Pack a slot's extents the way the vertex shader reads them: width in
    /// the low half, height in the high half.
    pub(crate) fn dim(width: u16, height: u16) -> u32 {
        u32::from(width) | (u32::from(height) << 16)
    }
}

/// Offset of `[color_atlas_size, mask_atlas_size]` in the shared
/// immediate region: straight after the viewport, which is what
/// `shader.wgsl` declares as `Immediates { viewport_size, atlas_px }`
/// — flat members, for the Dx12 constant-buffer reason documented there.
///
/// Derived rather than written as `8`, because the offset and
/// `ViewportPush`'s size are the same fact — a literal would let a field
/// added to the viewport silently overlap these.
pub(crate) const PARAMS_OFFSET: u32 = ViewportPush::BYTES as u32;

/// Pack an atlas slot's origin plus its content type into the one `u32` the
/// vertex shader unpacks: `u` in the low 15 bits, the type in bit 15, `v` in
/// the high 16.
pub(crate) fn pack_uv(u: u16, v: u16, kind: ContentType) -> u32 {
    debug_assert!(u <= 0x7FFF, "uv high bit reserved for content_type");
    (u as u32) | ((kind as u32) << 15) | ((v as u32) << 16)
}

const RASTER_QUAD_ATTRS: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
    0 => Sint32x2,
    1 => Uint32,
    2 => Uint32,
    3 => Unorm8x4,
];

// Compile-time guard: attribute offsets must match the struct fields they
// feed. `array_stride == size_of` alone wouldn't catch a same-size field
// reorder; `offset_of!` does. Matches the guards on the quad / mesh / image
// / curve pipelines.
const _: () = {
    use std::mem::offset_of;
    assert!(RASTER_QUAD_ATTRS[0].offset == offset_of!(RasterQuad, pos) as u64);
    assert!(RASTER_QUAD_ATTRS[1].offset == offset_of!(RasterQuad, dim) as u64);
    assert!(RASTER_QUAD_ATTRS[2].offset == offset_of!(RasterQuad, uv_and_kind) as u64);
    assert!(RASTER_QUAD_ATTRS[3].offset == offset_of!(RasterQuad, color) as u64);
};

pub(crate) fn instance_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<RasterQuad>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &RASTER_QUAD_ATTRS,
    }
}

/// The shader both passes build their pipelines from.
pub(crate) fn shader_module(device: &wgpu::Device, label: &str) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
    })
}

/// Group 0: mask at 0, colour at 1, one shared sampler at 2 — the same entry
/// shapes every other group uses, two textures deep instead of one.
pub(crate) fn bind_group_layout(device: &wgpu::Device, label: &str) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &[
            pipeline_utils::fragment_texture_entry(0),
            pipeline_utils::fragment_texture_entry(1),
            pipeline_utils::fragment_sampler_entry(2),
        ],
    })
}

/// Nearest on both axes: a quad is drawn at its slot's own pixel dimensions,
/// so every texel maps 1:1 and filtering could only blur what the rasterizer
/// already got exactly right.
pub(crate) fn sampler(device: &wgpu::Device, label: &str) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some(label),
        min_filter: wgpu::FilterMode::Nearest,
        mag_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    })
}

pub(crate) fn bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    mask_view: &wgpu::TextureView,
    color_view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    label: &str,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(mask_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(color_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

#[cfg(test)]
mod tests {
    use crate::renderer::backend::raster_atlas::ContentType;
    use crate::renderer::backend::raster_atlas::quad::{PARAMS_OFFSET, RasterQuad, pack_uv};
    use std::mem::{align_of, offset_of, size_of};

    /// The GPU wire format. Pinned here rather than in either pass, because
    /// both draw through it and neither owns it.
    #[test]
    fn raster_quad_is_20_bytes() {
        assert_eq!(size_of::<RasterQuad>(), 20);
        assert_eq!(align_of::<RasterQuad>(), 4);
        assert_eq!(offset_of!(RasterQuad, pos), 0);
        assert_eq!(offset_of!(RasterQuad, dim), 8);
        assert_eq!(offset_of!(RasterQuad, uv_and_kind), 12);
        assert_eq!(offset_of!(RasterQuad, color), 16);
    }

    /// The viewport and the atlas sizes share one immediate region, and the
    /// shader reads them as `Immediates { viewport, params }`. What a wider
    /// viewport or a third params field can still break is the pair fitting
    /// inside the region at all, which nothing else checks.
    #[test]
    fn viewport_and_params_fit_the_immediate_region() {
        use crate::renderer::backend::IMMEDIATES_BYTES;
        assert!(PARAMS_OFFSET as usize + size_of::<[u32; 2]>() <= IMMEDIATES_BYTES as usize);
    }

    #[test]
    fn pack_uv_round_trip() {
        let p = pack_uv(12345, 54321, ContentType::Color);
        assert_eq!(p & 0x7FFF, 12345);
        assert_eq!((p >> 15) & 1, 1);
        assert_eq!(p >> 16, 54321);

        let p = pack_uv(12345, 54321, ContentType::Mask);
        assert_eq!((p >> 15) & 1, 0);
    }

    /// `dim` is a packed pair, so a swapped shift shows up as a swapped box.
    #[test]
    fn dim_packs_width_low_height_high() {
        assert_eq!(RasterQuad::dim(7, 9), 7 | (9 << 16));
    }
}
