//! The off-screen color target the backbuffer-copy path renders into.

use crate::gpu::pipeline::blit_pipeline;
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::surface::render_target::RenderTarget;
use glam::UVec2;

/// Persistent off-screen color target for the backbuffer-copy path; keeping
/// last frame's pixels is what lets `LoadOp::Load` work for incremental damage.
/// Recreated on resize or format change; owned per-window by `WindowDriver`.
#[derive(Debug)]
pub(crate) struct Backbuffer {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
}

#[derive(Debug)]
pub(crate) struct EnsuredBackbuffer<'a> {
    pub(crate) backbuffer: &'a Backbuffer,
    /// A fresh texture is undefined until written, so a recreate requires a
    /// `Full` damage plan; upstream already forces it, so the caller asserts.
    pub(crate) recreated: bool,
}

impl Backbuffer {
    /// Copies this backbuffer's pixels onto `surface_texture`, which needs `COPY_DST` and must [`describe`](Self::describes) this backbuffer.
    pub(crate) fn copy_onto(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        surface_texture: &wgpu::Texture,
    ) {
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: surface_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            self.texture.size(),
        );
    }

    /// A backbuffer [`describing`](Self::describes) `target`. `binding` is what
    /// targets without copy support draw through.
    pub(crate) fn new(
        device: &wgpu::Device,
        binding: &TextureBinding,
        target: RenderTarget<'_>,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("palantir.renderer.backbuffer"),
            size: target.texture().size(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: target.texture().format(),
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = binding.bind_group(device, &view, "palantir.renderer.backbuffer.bg");
        Self {
            texture,
            view,
            bind_group,
        }
    }

    /// Draws this backbuffer onto a target that cannot be copied into, in its
    /// own pass since it replaces the target.
    pub(crate) fn draw_onto(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        pipeline: &wgpu::RenderPipeline,
    ) {
        blit_pipeline::draw(encoder, &self.bind_group, target, pipeline);
    }

    pub(crate) const fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    pub(crate) const fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }

    /// Width and height in physical pixels.
    pub(crate) fn size(&self) -> UVec2 {
        let size = self.texture.size();
        UVec2::new(size.width, size.height)
    }

    /// Whether this backbuffer matches `target`'s size and format.
    pub(crate) fn describes(&self, target: RenderTarget<'_>) -> bool {
        let texture = target.texture();
        self.texture.size() == texture.size() && self.texture.format() == texture.format()
    }
}
