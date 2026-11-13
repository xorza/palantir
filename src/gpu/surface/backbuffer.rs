//! The off-screen color target the backbuffer-copy path renders into.

use crate::gpu::device::backbuffer_out::BackbufferOut;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::pipeline::blit_pipeline;
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::surface::owned_target::TargetSpec;
use crate::gpu::surface::render_target::RenderTarget;
use crate::gpu::texel;
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
}

impl Backbuffer {
    /// Put this backbuffer's pixels on `target`, which it must [`fit`](Self::fits):
    /// a copy where the target takes one and the device copies (`out`), else a draw through
    /// `blit`, in a pass of its own as it replaces the target, onto the target's view.
    pub(crate) fn present_onto(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: RenderTarget<'_>,
        blit: &wgpu::RenderPipeline,
        out: BackbufferOut,
    ) {
        debug_assert!(
            self.fits(target),
            "a backbuffer presents onto a target of its size and format, as the frame that \
             painted it, or the frame a skip follows, saw"
        );
        if out == BackbufferOut::Copy && target.takes_copy() {
            encoder.copy_texture_to_texture(
                self.texture.as_image_copy(),
                target.texture().as_image_copy(),
                self.texture.size(),
            );
        } else {
            blit_pipeline::draw(encoder, &self.bind_group, &target.view(), blit);
        }
    }

    /// A backbuffer [`fitting`](Self::fits) `target`. `binding` is what
    /// targets without copy support draw through.
    pub(crate) fn new(gpu: &Gpu, binding: &TextureBinding, target: RenderTarget<'_>) -> Self {
        let texture = texel::texture_2d(
            gpu,
            TargetSpec {
                label: "palantir.renderer.backbuffer",
                size: texel::size(target.texture()),
                format: target.texture().format(),
                // `COPY_DST` takes a damage canvas's rects.
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::TEXTURE_BINDING,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = binding.bind_group(gpu, &view, "palantir.renderer.backbuffer.bg");
        Self {
            texture,
            view,
            bind_group,
        }
    }

    pub(crate) const fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    pub(crate) const fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }

    /// Width and height in physical pixels.
    pub(crate) fn size(&self) -> UVec2 {
        texel::size(&self.texture)
    }

    /// Whether this backbuffer matches `target`'s size and format.
    pub(crate) fn fits(&self, target: RenderTarget<'_>) -> bool {
        let texture = target.texture();
        self.texture.size() == texture.size() && self.texture.format() == texture.format()
    }
}
