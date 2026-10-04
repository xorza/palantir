//! The instance a [`RasterAtlas`](crate::gpu::raster::raster_atlas::RasterAtlas)
//! is drawn through, and the shader and vertex layout that read it.
//!
//! Both tenants draw the same rectangle — a tinted quad sampling one atlas
//! slot — so the instance, the shader, and the group-0 layout live here with
//! the atlas rather than inside whichever pass happened to need them first.
//! What differs between the passes is only which atlas they bind and where
//! their pixels came from.

use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::content_type::ContentType;

/// One per-instance vertex record. 28 bytes, `Pod`.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct RasterQuad {
    /// Top-left in physical px.
    pub(crate) pos: [i32; 2],
    /// The raster's extents in atlas texels.
    pub(crate) dim: [u16; 2],
    /// The extents the quad is drawn at in physical px. Equal to `dim` for
    /// every glyph and for an icon in the exact band, which the shader
    /// reads texel for texel; any other icon is resampled to its box.
    pub(crate) size: [u16; 2],
    /// Atlas origin plus content type, packed by [`Self::pack_uv`].
    pub(crate) uv_and_kind: u32,
    /// Straight-alpha linear RGBA; the shader premultiplies at output.
    pub(crate) color: RgbaF16,
}

impl RasterQuad {
    /// Collapse a colour raster to its luminance when drawn — OR into the
    /// value [`Self::pack_uv`] returns.
    ///
    /// The disabled look for a **colour** icon, whose own colours a tint cannot
    /// replace (the colour path ignores tint RGB and takes only its alpha). Has
    /// no effect on the mask path, where the draw already chooses the colour
    /// outright.
    pub(crate) const DESATURATE: u32 = 1 << U_BITS;

    /// Pack an atlas slot's origin plus its content type into the one `u32` the
    /// vertex shader unpacks: `u` in the low [`U_BITS`], [`Self::DESATURATE`]
    /// above it, the content type above that, `v` from [`V_SHIFT`] up.
    pub(crate) fn pack_uv(u: u16, v: u16, kind: ContentType) -> u32 {
        debug_assert!(
            u32::from(u) <= U_MAX,
            "u must fit {U_BITS} bits; the rest carry the content type and DESATURATE",
        );
        u32::from(u) | ((kind as u32) << KIND_SHIFT) | (u32::from(v) << V_SHIFT)
    }

    /// The vertex layout the instance stream is read through.
    pub(crate) const fn instance_layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &RASTER_QUAD_ATTRS,
        }
    }
}

/// Bits of `uv_and_kind` that hold `u`, and so the shift the two flags sit at.
///
/// Fourteen is more than either side can use: the byte budget caps a mask
/// atlas at 4096 and a colour atlas at 2048, both inside 12 bits. Every other
/// number in the layout derives from this one — including the shader's, which
/// `ShaderBody::RasterAtlas` substitutes rather than restates.
pub(crate) const U_BITS: u32 = 14;

/// Largest `u` the layout can carry.
const U_MAX: u32 = (1 << U_BITS) - 1;

/// Where the content type sits: straight above [`RasterQuad::DESATURATE`].
const KIND_SHIFT: u32 = U_BITS + 1;

/// Where `v` starts: the upper half, which holds any `u16`.
pub(crate) const V_SHIFT: u32 = 16;

/// The carried flags — [`RasterQuad::DESATURATE`] and the content type —
/// once shifted down by [`U_BITS`]: every bit between `u` and `v`.
pub(crate) const FLAG_MASK: u32 = (1 << (V_SHIFT - U_BITS)) - 1;

/// [`RasterQuad::DESATURATE`] as the shader reads it, shifted down by
/// [`U_BITS`].
pub(crate) const FLAG_DESATURATE: u32 = RasterQuad::DESATURATE >> U_BITS;

/// A colour raster's content-type bit as the shader reads it, shifted down
/// by [`U_BITS`].
pub(crate) const FLAG_COLOR: u32 = (ContentType::Color as u32) << (KIND_SHIFT - U_BITS);

// Compile-time guard on the layout: the three fields must tile the `u32`
// without overlapping, so both carried flags fall inside `FLAG_MASK`.
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

// Compile-time guard: attribute offsets must match the struct fields they
// feed. `array_stride == size_of` alone wouldn't catch a same-size field
// reorder; `offset_of!` does. Matches the guards on the quad / mesh / image
// / curve pipelines.
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

    /// The GPU wire format. Pinned here rather than in either pass, because
    /// both draw through it and neither owns it.
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

        // What the shader reads: the two flag bits between `u` (14 bits)
        // and `v` (from bit 16), content type the upper one.
        assert_eq!(FLAG_MASK, 0b11);
        assert_eq!((p >> U_BITS) & FLAG_MASK, 0b10);

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
}
