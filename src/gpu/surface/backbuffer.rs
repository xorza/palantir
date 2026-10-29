//! The off-screen color target the backbuffer-copy path renders into.

use crate::gpu::backdrop::backdrop_blur::TargetSource;
use crate::gpu::pipeline::blit_pipeline;
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::surface::render_target::{self, RenderTarget};
use glam::UVec2;

/// Persistent off-screen color target for the backbuffer-copy path; keeping
/// last frame's pixels is what lets `LoadOp::Load` work for incremental damage.
/// Recreated on resize or format change; its window's
/// [`RenderStream`](crate::gpu::wgpu_backend::render_stream::RenderStream) keeps it.
#[derive(Debug)]
pub(crate) struct Backbuffer {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
    /// The group a pane's blur reads this backbuffer through as its level 0, made on the first
    /// frame that blurs from it and dropped with it.
    blur_source: Option<TargetSource>,
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
            blur_source: None,
        }
    }

    /// The source a pane's blur reads this backbuffer through, made by `make` from its view and
    /// extent on the first call.
    pub(crate) fn blur_source(
        &mut self,
        make: impl FnOnce(&wgpu::TextureView, UVec2) -> TargetSource,
    ) -> TargetSource {
        let size = self.size();
        self.blur_source
            .get_or_insert_with(|| make(&self.view, size))
            .clone()
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
        render_target::texture_size(&self.texture)
    }

    /// Whether this backbuffer matches `target`'s size and format.
    pub(crate) fn describes(&self, target: RenderTarget<'_>) -> bool {
        let texture = target.texture();
        self.texture.size() == texture.size() && self.texture.format() == texture.format()
    }
}
