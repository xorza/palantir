//! The off-screen colour target the backbuffer-copy path renders into.

use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::surface::render_target::{self, TargetFormat};
use glam::UVec2;

/// Persistent off-screen colour target for the backbuffer-copy path; keeping
/// last frame's pixels is what lets `LoadOp::Load` work for incremental damage.
/// Recreated on resize or format change; owned per-window by `WindowDriver`.
#[derive(Debug)]
pub(crate) struct Backbuffer {
    tex: wgpu::Texture,
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
    /// The window's backbuffer at `size` and `format`, built if the slot is empty
    /// or no longer [`describes`](Self::describes) the target. `binding` is what
    /// targets without copy support draw through.
    pub(crate) fn ensure<'s>(
        slot: &'s mut Option<Self>,
        device: &wgpu::Device,
        binding: &TextureBinding,
        size: UVec2,
        format: TargetFormat,
    ) -> EnsuredBackbuffer<'s> {
        let size = render_target::extent(size);
        let format = format.get();
        if slot
            .as_ref()
            .is_some_and(|held| !held.describes(size, format))
        {
            *slot = None;
        }
        let recreated = slot.is_none();
        EnsuredBackbuffer {
            backbuffer: slot.get_or_insert_with(|| Self::new(device, binding, size, format)),
            recreated,
        }
    }

    /// Copies this backbuffer's pixels onto `surface_tex`, which needs `COPY_DST` and must [`describe`](Self::describes) this backbuffer.
    pub(crate) fn copy_onto(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        surface_tex: &wgpu::Texture,
    ) {
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: surface_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            self.tex.size(),
        );
    }

    fn new(
        device: &wgpu::Device,
        binding: &TextureBinding,
        size: wgpu::Extent3d,
        format: wgpu::TextureFormat,
    ) -> Self {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("palantir.renderer.backbuffer"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = binding.bind_group(device, &view, "palantir.renderer.backbuffer.bg");
        Self {
            tex,
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
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("palantir.renderer.blit"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    // Clear, though every texel is written: on tilers a clear skips loading tile memory.
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    pub(crate) const fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// Whether this backbuffer matches a target of `size` and `format`.
    pub(crate) fn describes(&self, size: wgpu::Extent3d, format: wgpu::TextureFormat) -> bool {
        self.tex.size() == size && self.tex.format() == format
    }
}
