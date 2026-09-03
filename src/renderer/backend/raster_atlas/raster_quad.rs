//! The instance a [`RasterAtlas`](crate::renderer::backend::raster_atlas::RasterAtlas)
//! is drawn through, and the shader and vertex layout that read it.
//!
//! Both tenants draw the same rectangle — a tinted quad sampling one atlas
//! slot — so the instance, the shader, and the group-0 layout live here with
//! the atlas rather than inside whichever pass happened to need them first.
//! What differs between the passes is only which atlas they bind and where
//! their pixels came from.

use crate::primitives::content_type::ContentType;
use crate::renderer::backend::shader_template::{self, ShaderConstant};
use crate::renderer::backend::viewport::ViewportPush;

/// One per-instance vertex record. 20 bytes, `Pod`.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct RasterQuad {
    /// Top-left in physical px.
    pub(crate) pos: [i32; 2],
    /// Extents, packed by [`Self::dim`].
    pub(crate) dim: u32,
    /// Atlas origin plus content type, packed by [`Self::pack_uv`].
    pub(crate) uv_and_kind: u32,
    /// Straight-alpha linear RGBA; the shader premultiplies at output.
    pub(crate) color: u32,
}

impl RasterQuad {
    /// Offset of `[color_atlas_size, mask_atlas_size]` in the shared
    /// immediate region: straight after the viewport, which is what
    /// `shader.wgsl` declares as `Immediates { viewport_size, atlas_px }`
    /// — flat members, for the Dx12 constant-buffer reason documented there.
    ///
    /// Derived rather than written as `8`, because the offset and
    /// `ViewportPush`'s size are the same fact — a literal would let a field
    /// added to the viewport silently overlap these.
    pub(crate) const PARAMS_OFFSET: u32 = ViewportPush::BYTES as u32;

    /// Collapse a colour raster to its luminance when drawn — OR into the
    /// value [`Self::pack_uv`] returns.
    ///
    /// The disabled look for a **colour** icon, whose own colours a tint cannot
    /// replace (the colour path ignores tint RGB and takes only its alpha). Has
    /// no effect on the mask path, where the draw already chooses the colour
    /// outright.
    pub(crate) const DESATURATE: u32 = 1 << U_BITS;

    /// Pack a slot's extents the way the vertex shader reads them: width in
    /// the low half, height in the high half.
    pub(crate) fn dim(width: u16, height: u16) -> u32 {
        u32::from(width) | (u32::from(height) << 16)
    }

    /// Pack an atlas slot's origin plus its content type into the one `u32` the
    /// vertex shader unpacks: `u` in the low [`U_BITS`], [`Self::DESATURATE`]
    /// above it, the content type above that, `v` in the high 16.
    pub(crate) fn pack_uv(u: u16, v: u16, kind: ContentType) -> u32 {
        debug_assert!(
            u32::from(u) <= U_MAX,
            "u must fit {U_BITS} bits; the rest carry the content type and DESATURATE",
        );
        (u as u32) | ((kind as u32) << KIND_SHIFT) | ((v as u32) << 16)
    }

    /// The vertex layout the instance stream is read through.
    pub(crate) fn instance_layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &RASTER_QUAD_ATTRS,
        }
    }

    /// The shader both passes build their pipelines from.
    ///
    /// Rust owns the `uv_and_kind` bit layout; the shader declares the three
    /// numbers it needs as markers so the two cannot drift (`specialize` panics
    /// on an unsubstituted one). The flags arrive already shifted down by
    /// [`U_BITS`], which is how the shader reads them.
    pub(crate) fn shader_module(device: &wgpu::Device, label: &str) -> wgpu::ShaderModule {
        let wgsl = shader_template::specialize(
            shader_template::RASTER_ATLAS_WGSL,
            &[
                ShaderConstant::uint("U_BITS", U_BITS),
                ShaderConstant::uint("FLAG_DESATURATE", Self::DESATURATE >> U_BITS),
                ShaderConstant::uint(
                    "FLAG_COLOR",
                    (ContentType::Color as u32) << (KIND_SHIFT - U_BITS),
                ),
            ],
        );
        device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(wgsl.into()),
        })
    }
}

/// Bits of `uv_and_kind` that hold `u`, and so the shift the two flags sit at.
///
/// Fourteen is more than either side can use: the byte budget caps a mask
/// atlas at 4096 and a colour atlas at 2048, both inside 12 bits. Every other
/// number in the layout derives from this one — including the shader's, which
/// [`RasterQuad::shader_module`] substitutes rather than restates.
const U_BITS: u32 = 14;

/// Largest `u` the layout can carry.
const U_MAX: u32 = (1 << U_BITS) - 1;

/// Where the content type sits: straight above [`RasterQuad::DESATURATE`].
const KIND_SHIFT: u32 = U_BITS + 1;

// Compile-time guard on the layout: the three fields must tile the `u32`
// without overlapping, and the shader reads the flags as a two-bit field
// directly above `u` — so the values substituted into the WGSL have to come
// out as exactly 1 and 2. Same shape as the vertex-attribute guard below.
const _: () = {
    assert!(
        RasterQuad::DESATURATE >> U_BITS == 1,
        "shader's FLAG_DESATURATE"
    );
    assert!(
        (ContentType::Color as u32) << (KIND_SHIFT - U_BITS) == 2,
        "shader's FLAG_COLOR",
    );
    // `v` starts at bit 16, so neither flag may reach it.
    assert!(KIND_SHIFT < 16);
};

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

#[cfg(test)]
mod tests {
    use crate::primitives::content_type::ContentType;
    use crate::renderer::backend::raster_atlas::raster_quad::{RasterQuad, U_MAX};
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
        assert!(
            RasterQuad::PARAMS_OFFSET as usize + size_of::<[u32; 2]>() <= IMMEDIATES_BYTES as usize
        );
    }

    /// The three fields share one `u32`, so each has to survive the other
    /// two. `u` is taken at the top of its range to catch a mask that is one
    /// bit too wide.
    #[test]
    fn pack_uv_round_trip() {
        let p = RasterQuad::pack_uv(U_MAX as u16, 54321, ContentType::Color);
        assert_eq!(p & U_MAX, U_MAX);
        assert_eq!((p >> 15) & 1, 1);
        assert_eq!(p >> 16, 54321);
        assert_eq!(
            p & RasterQuad::DESATURATE,
            0,
            "not desaturated unless asked"
        );

        let p = RasterQuad::pack_uv(12345, 54321, ContentType::Mask);
        assert_eq!((p >> 15) & 1, 0);
        assert_eq!(p & U_MAX, 12345);

        // The flag rides above `u` and below the content type, so setting it
        // must disturb neither.
        let p =
            RasterQuad::pack_uv(U_MAX as u16, 54321, ContentType::Color) | RasterQuad::DESATURATE;
        assert_eq!(p & U_MAX, U_MAX);
        assert_eq!((p >> 15) & 1, 1);
        assert_eq!(p >> 16, 54321);
        assert_ne!(p & RasterQuad::DESATURATE, 0);
    }

    /// `dim` is a packed pair, so a swapped shift shows up as a swapped box.
    #[test]
    fn dim_packs_width_low_height_high() {
        assert_eq!(RasterQuad::dim(7, 9), 7 | (9 << 16));
    }
}
