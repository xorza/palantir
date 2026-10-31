//! [`DamageCanvas`]: the small color target a partial frame paints its damage into on a device
//! whose pass loads and stores every tile of its attachments
//! ([`PassTraffic::WholeTarget`](crate::gpu::device::pass_traffic::PassTraffic::WholeTarget)).

use crate::gpu::surface::render_target;
use crate::gpu::surface::stencil::Stencil;
use crate::primitives::geometry::urect::URect;
use glam::UVec2;

/// The side a canvas's rect and its size snap out to: the largest tile V3D and Mali use, so the
/// rect's tiles are whole on either.
const TILE: u32 = 64;

/// A color target, and a stencil from its first frame with a rounded clip, of the
/// [size class](Self::class) of the damage a partial frame repaints. The frame paints into it from a clear, as a whole frame paints the backbuffer, then
/// copies each damage rect into the backbuffer: such a device's pass then loads nothing and stores
/// the canvas alone, where a pass on the backbuffer loads and stores the whole surface.
#[derive(Debug)]
pub(crate) struct DamageCanvas {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    stencil: Option<Stencil>,
}

impl DamageCanvas {
    /// A canvas of `size` in `format`.
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat, size: UVec2) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("palantir.renderer.damage_canvas"),
            size: render_target::extent(size),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture,
            view,
            stencil: None,
        }
    }

    /// The surface rect a partial frame of `damage` paints through a canvas on `surface`: the
    /// damage's bounds out to whole [`TILE`]s, inside the surface. `None` where the canvas would
    /// hold more than half the surface: its store and the rects' copies then cost about what a
    /// pass on the backbuffer loads and stores.
    pub(crate) fn rect(damage: URect, surface: UVec2) -> Option<URect> {
        let min = damage.min / TILE * TILE;
        let max = damage.max().map(|side| side.div_ceil(TILE) * TILE);
        let rect = URect::from_min_max(min, max.min(surface));
        (rect.area() * 2 <= u64::from(surface.x) * u64::from(surface.y)).then_some(rect)
    }

    /// The size of the canvas that holds `rect`: each side out to a power of two from [`TILE`] up,
    /// inside the surface, so the damage of a run of frames shares a few canvases.
    pub(crate) fn class(rect: URect, surface: UVec2) -> UVec2 {
        let pow2 = UVec2::new(
            rect.size.x.next_power_of_two(),
            rect.size.y.next_power_of_two(),
        );
        pow2.max(UVec2::splat(TILE)).min(surface).max(rect.size)
    }

    /// Copy the canvas's pixels of `rect`, a surface rect, into `backbuffer` at `rect`, for a
    /// canvas whose pixel 0 is the surface's `origin`.
    pub(crate) fn copy_into(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        backbuffer: &wgpu::Texture,
        rect: URect,
        origin: UVec2,
    ) {
        let at = rect.min - origin;
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: at.x,
                    y: at.y,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: backbuffer,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: rect.min.x,
                    y: rect.min.y,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            render_target::extent(rect.size),
        );
    }

    /// Whether this canvas is `size` in `format`.
    pub(crate) fn fits(&self, size: UVec2, format: wgpu::TextureFormat) -> bool {
        render_target::texture_size(&self.texture) == size && self.texture.format() == format
    }

    pub(crate) const fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }

    pub(crate) const fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// The stencil [`Self::ensure_stencil`] made.
    pub(crate) const fn stencil(&self) -> Option<&Stencil> {
        self.stencil.as_ref()
    }

    /// Make the stencil when there is none.
    pub(crate) fn ensure_stencil(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        if self.stencil.is_none() {
            self.stencil = Some(Stencil::new(
                device,
                queue,
                render_target::texture_size(&self.texture),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::surface::damage_canvas::DamageCanvas;
    use crate::primitives::geometry::urect::URect;
    use glam::UVec2;

    /// The damage (100, 70) + 30 × 20 snaps out to (64, 64)..(192, 128), 128 × 64 of a
    /// 2560 × 1440 surface, whose class is 128 × 64. Damage at the surface's corner snaps inside
    /// it: (2550, 1430) + 10 × 10 to (2496, 1408)..(2560, 1440), 64 × 32, in a class of 64 × 64.
    /// A canvas past half the surface paints none: 1920 × 1080 snaps to 1920 × 1088, 57 %. A
    /// 300 × 40 rect snaps to 320 × 64 and takes a 512 × 64 class.
    #[test]
    fn a_canvas_holds_the_damage_out_to_whole_tiles() {
        let surface = UVec2::new(2560, 1440);
        let rect = DamageCanvas::rect(URect::new(100, 70, 30, 20), surface).unwrap();
        assert_eq!(rect, URect::new(64, 64, 128, 64));
        assert_eq!(DamageCanvas::class(rect, surface), UVec2::new(128, 64));
        let corner = DamageCanvas::rect(URect::new(2550, 1430, 10, 10), surface).unwrap();
        assert_eq!(corner, URect::new(2496, 1408, 64, 32));
        assert_eq!(DamageCanvas::class(corner, surface), UVec2::new(64, 64));
        assert_eq!(
            DamageCanvas::rect(URect::new(0, 0, 1920, 1080), surface),
            None
        );
        let wide = DamageCanvas::rect(URect::new(0, 0, 300, 40), surface).unwrap();
        assert_eq!(wide, URect::new(0, 0, 320, 64));
        assert_eq!(DamageCanvas::class(wide, surface), UVec2::new(512, 64));
    }
}
