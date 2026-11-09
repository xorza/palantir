//! The instance a `RasterAtlas` of `gpu::raster` is drawn through, with the vertex layout that
//! reads it.

use crate::gpu::pipeline::pipeline_recipe::{self, vertex_attr};
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::content_type::ContentType;
use glam::{IVec2, U16Vec2, UVec2, Vec2};
use std::mem::offset_of;

/// One per-instance vertex record. 32 bytes, `Pod`.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct RasterQuad {
    pub(crate) pos: IVec2,
    pub(crate) dim: U16Vec2,
    /// The drawn extents in physical px; equal to `dim` except for resampled icons. `u32`, as a
    /// resampled icon's quad is its whole box, which a zoom takes past 65535 px; exact to 2²⁴ px,
    /// where the shader's f32 texel mapping stops being exact.
    pub(crate) size: UVec2,
    /// Atlas origin plus content type, packed by [`Self::pack_uv`].
    pub(crate) uv_and_kind: u32,
    /// Straight-alpha linear RGBA; the shader premultiplies at output.
    pub(crate) color: RgbaF16,
}

impl RasterQuad {
    /// Collapses a color raster to luminance when OR-ed into [`Self::pack_uv`]'s
    /// value: the disabled look for a color icon. No effect on the mask path.
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

    /// This quad's part inside the physical px from `min` to `max`, its atlas origin moved by the
    /// texels cut off; `None` where none of it lies inside. A quad drawn at its raster's own size,
    /// a texel a pixel, as a glyph is, so the cut is exact.
    pub(crate) fn clipped(self, min: IVec2, max: IVec2) -> Option<Self> {
        debug_assert_eq!(
            self.size,
            self.dim.as_uvec2(),
            "a clipped quad is drawn at its raster's own size"
        );
        let lo = self.pos.max(min);
        let hi = (self.pos + self.size.as_ivec2()).min(max);
        if lo.x >= hi.x || lo.y >= hi.y {
            return None;
        }
        let cut = (lo - self.pos).as_uvec2();
        let size = (hi - lo).as_uvec2();
        Some(Self {
            pos: lo,
            dim: size.as_u16vec2(),
            size,
            // `u` and `v` move by the cut, which stays inside the slot, so neither carries into
            // the field past it.
            uv_and_kind: self.uv_and_kind + cut.x + (cut.y << V_SHIFT),
            color: self.color,
        })
    }

    pub(crate) const LAYOUT: wgpu::VertexBufferLayout<'static> = pipeline_recipe::vertex_layout(
        size_of::<Self>(),
        wgpu::VertexStepMode::Instance,
        &RASTER_QUAD_ATTRS,
    );
}

/// Bits of `uv_and_kind` that hold `u`, and the shift the flags sit at. An atlas side grows no
/// wider than `1 << U_BITS`; everything else, including the shader's copy
/// (`RasterProgram::SHADER`), derives from this.
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

/// A [`RasterQuad`] a paint animation turns: the quad as drawn unturned, and the turn of its whole
/// run about the physical `pivot`, as `rotor = (cos, sin)`. In a buffer of its own, so an
/// unturned glyph keeps its 32 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct SpunRasterQuad {
    pub(crate) quad: RasterQuad,
    pub(crate) pivot: Vec2,
    pub(crate) rotor: Vec2,
}

impl SpunRasterQuad {
    pub(crate) const LAYOUT: wgpu::VertexBufferLayout<'static> = pipeline_recipe::vertex_layout(
        size_of::<Self>(),
        wgpu::VertexStepMode::Instance,
        &SPUN_RASTER_QUAD_ATTRS,
    );
}

const RASTER_QUAD_ATTRS: [wgpu::VertexAttribute; 5] = [
    vertex_attr(0, wgpu::VertexFormat::Sint32x2, offset_of!(RasterQuad, pos)),
    vertex_attr(1, wgpu::VertexFormat::Uint16x2, offset_of!(RasterQuad, dim)),
    vertex_attr(
        2,
        wgpu::VertexFormat::Uint32x2,
        offset_of!(RasterQuad, size),
    ),
    vertex_attr(
        3,
        wgpu::VertexFormat::Uint32,
        offset_of!(RasterQuad, uv_and_kind),
    ),
    vertex_attr(
        4,
        wgpu::VertexFormat::Float16x4,
        offset_of!(RasterQuad, color),
    ),
];

const SPUN_RASTER_QUAD_ATTRS: [wgpu::VertexAttribute; 7] = [
    vertex_attr(
        0,
        wgpu::VertexFormat::Sint32x2,
        offset_of!(SpunRasterQuad, quad.pos),
    ),
    vertex_attr(
        1,
        wgpu::VertexFormat::Uint16x2,
        offset_of!(SpunRasterQuad, quad.dim),
    ),
    vertex_attr(
        2,
        wgpu::VertexFormat::Uint32x2,
        offset_of!(SpunRasterQuad, quad.size),
    ),
    vertex_attr(
        3,
        wgpu::VertexFormat::Uint32,
        offset_of!(SpunRasterQuad, quad.uv_and_kind),
    ),
    vertex_attr(
        4,
        wgpu::VertexFormat::Float16x4,
        offset_of!(SpunRasterQuad, quad.color),
    ),
    vertex_attr(
        5,
        wgpu::VertexFormat::Float32x2,
        offset_of!(SpunRasterQuad, pivot),
    ),
    vertex_attr(
        6,
        wgpu::VertexFormat::Float32x2,
        offset_of!(SpunRasterQuad, rotor),
    ),
];

#[cfg(test)]
mod tests {
    use crate::gpu::pipeline::raster_pipeline::raster_quad::{
        FLAG_MASK, RasterQuad, U_BITS, U_MAX,
    };
    use crate::primitives::paint::color::rgba_f16::RgbaF16;
    use crate::primitives::paint::content_type::ContentType;

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

    /// A quad keeps its part inside the clip, its atlas origin moved by the texels cut off: a
    /// 10 × 8 glyph at (5, 4), atlas (100, 200), against a clip from (8, 6) to (12, 20) keeps
    /// (8, 6), 4 × 6, at atlas (103, 202). A quad inside the clip stays as it was, one that touches
    /// it only along an edge goes, and the content type rides through.
    #[test]
    fn a_clipped_quad_keeps_its_texels_inside() {
        use glam::{IVec2, U16Vec2, UVec2};

        let quad = RasterQuad {
            pos: IVec2::new(5, 4),
            dim: U16Vec2::new(10, 8),
            size: UVec2::new(10, 8),
            uv_and_kind: RasterQuad::pack_uv(100, 200, ContentType::Color),
            color: RgbaF16::WHITE,
        };
        let cut = quad
            .clipped(IVec2::new(8, 6), IVec2::new(12, 20))
            .expect("the clip holds part of it");
        assert_eq!(cut.pos, IVec2::new(8, 6));
        assert_eq!(cut.dim, U16Vec2::new(4, 6));
        assert_eq!(cut.size, UVec2::new(4, 6));
        assert_eq!(
            cut.uv_and_kind,
            RasterQuad::pack_uv(103, 202, ContentType::Color)
        );
        let inside = quad.clipped(IVec2::ZERO, IVec2::splat(100)).unwrap();
        assert_eq!(
            (inside.pos, inside.dim, inside.size, inside.uv_and_kind),
            (quad.pos, quad.dim, quad.size, quad.uv_and_kind)
        );
        assert!(quad.clipped(IVec2::new(15, 0), IVec2::splat(100)).is_none());
        assert!(quad.clipped(IVec2::ZERO, IVec2::new(100, 4)).is_none());
    }
}
