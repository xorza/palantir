//! The instance a [`RasterAtlas`](crate::gpu::raster::raster_atlas::RasterAtlas)
//! is drawn through, with the shader and vertex layout that read it.

use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::content_type::ContentType;

/// One per-instance vertex record. 28 bytes, `Pod`.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct RasterQuad {
    pub(crate) pos: [i32; 2],
    pub(crate) dim: [u16; 2],
    /// The drawn extents in physical px; equal to `dim` except for resampled icons.
    pub(crate) size: [u16; 2],
    /// Atlas origin plus content type, packed by [`Self::pack_uv`].
    pub(crate) uv_and_kind: u32,
    /// Straight-alpha linear RGBA; the shader premultiplies at output.
    pub(crate) color: RgbaF16,
}

impl RasterQuad {
    /// Collapses a colour raster to luminance when OR-ed into [`Self::pack_uv`]'s
    /// value: the disabled look for a colour icon. No effect on the mask path.
    pub(crate) const DESATURATE: u32 = 1 << U_BITS;

    /// Packs an atlas slot's origin and content type into the `u32` the vertex
    /// shader unpacks: `u` low, then [`Self::DESATURATE`], the content type, `v` from [`V_SHIFT`].
    pub(crate) fn pack_uv(u: u16, v: u16, kind: ContentType) -> u32 {
        debug_assert!(
            u32::from(u) <= U_MAX,
            "u must fit {U_BITS} bits; the rest carry the content type and DESATURATE",
        );
        u32::from(u) | ((kind as u32) << KIND_SHIFT) | (u32::from(v) << V_SHIFT)
    }

    pub(crate) const fn instance_layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &RASTER_QUAD_ATTRS,
        }
    }
}

/// Bits of `uv_and_kind` that hold `u`, and the shift the flags sit at. Both
/// atlases fit in 12; everything else, including the shader's copy
/// (`ShaderBody::RasterAtlas`), derives from this.
pub(crate) const U_BITS: u32 = 14;

const U_MAX: u32 = (1 << U_BITS) - 1;

const KIND_SHIFT: u32 = U_BITS + 1;

pub(crate) const V_SHIFT: u32 = 16;

pub(crate) const FLAG_MASK: u32 = (1 << (V_SHIFT - U_BITS)) - 1;

pub(crate) const FLAG_DESATURATE: u32 = RasterQuad::DESATURATE >> U_BITS;

pub(crate) const FLAG_COLOR: u32 = (ContentType::Color as u32) << (KIND_SHIFT - U_BITS);

const _: () = {
    assert!(
        (FLAG_DESATURATE | FLAG_COLOR) & !FLAG_MASK == 0,
        "a flag reaches `v`"
    );
    assert!(FLAG_DESATURATE & FLAG_COLOR == 0, "the two flags overlap");
};

const RASTER_QUAD_ATTRS: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
    0 => Sint32x2,
    1 => Uint16x2,
    2 => Uint16x2,
    3 => Uint32,
    4 => Float16x4,
];

// Guard: attribute offsets match the fields; `offset_of!` catches a same-size reorder.
const _: () = {
    use std::mem::offset_of;
    assert!(RASTER_QUAD_ATTRS[0].offset == offset_of!(RasterQuad, pos) as u64);
    assert!(RASTER_QUAD_ATTRS[1].offset == offset_of!(RasterQuad, dim) as u64);
    assert!(RASTER_QUAD_ATTRS[2].offset == offset_of!(RasterQuad, size) as u64);
    assert!(RASTER_QUAD_ATTRS[3].offset == offset_of!(RasterQuad, uv_and_kind) as u64);
    assert!(RASTER_QUAD_ATTRS[4].offset == offset_of!(RasterQuad, color) as u64);
};

#[cfg(test)]
mod tests {
    use crate::gpu::raster::raster_atlas::raster_quad::{FLAG_MASK, RasterQuad, U_BITS, U_MAX};
    use crate::primitives::paint::content_type::ContentType;
    use std::mem::offset_of;

    #[test]
    fn raster_quad_is_28_bytes() {
        assert_eq!(size_of::<RasterQuad>(), 28);
        assert_eq!(align_of::<RasterQuad>(), 4);
        assert_eq!(offset_of!(RasterQuad, pos), 0);
        assert_eq!(offset_of!(RasterQuad, dim), 8);
        assert_eq!(offset_of!(RasterQuad, size), 12);
        assert_eq!(offset_of!(RasterQuad, uv_and_kind), 16);
        assert_eq!(offset_of!(RasterQuad, color), 20);
    }

    /// The three fields share one `u32`; `u` is taken at its range top to catch a mask one bit too wide.
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

        assert_eq!(FLAG_MASK, 0b11);
        assert_eq!((p >> U_BITS) & FLAG_MASK, 0b10);

        let p = RasterQuad::pack_uv(12345, 54321, ContentType::Mask);
        assert_eq!((p >> 15) & 1, 0);
        assert_eq!(p & U_MAX, 12345);

        let p =
            RasterQuad::pack_uv(U_MAX as u16, 54321, ContentType::Color) | RasterQuad::DESATURATE;
        assert_eq!(p & U_MAX, U_MAX);
        assert_eq!((p >> 15) & 1, 1);
        assert_eq!(p >> 16, 54321);
        assert_ne!(p & RasterQuad::DESATURATE, 0);
    }
}
