//! Registered-image GPU bindings and their upload/drop lifecycle.

use crate::primitives::image::Image;
use crate::primitives::texture_id::TextureId;
use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::pipeline_utils::{texture_bind_group, texture_sampler_bgl};
use crate::renderer::backend::queue::Queue;
use crate::renderer::image_registry::ImageRegistry;
use rustc_hash::FxHashMap;

#[derive(Debug)]
pub(super) struct ImageTextures {
    pub(super) bindings: FxHashMap<TextureId, wgpu::BindGroup>,
    /// Group 0 layout (per-image texture + sampler). Built once; every
    /// bind group in `bindings` references it, and
    /// `ImagePipeline::build_variants` composes each format's pipeline
    /// layout against it — the only consumer outside this file.
    pub(super) bgl: wgpu::BindGroupLayout,
    /// Shared by every image and `GpuView` target: min/mag nearest
    /// filtering is a shader-side UV texel-center snap, so all filter
    /// combinations ride one sampler and one bind group.
    sampler: wgpu::Sampler,
}

impl ImageTextures {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        Self {
            bindings: FxHashMap::default(),
            bgl: texture_sampler_bgl(device, "palantir.image.tex.bgl"),
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("palantir.image.sampler"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Nearest,
                ..Default::default()
            }),
        }
    }

    #[profiling::function]
    pub(super) fn drain_registry(&mut self, ctx: &mut GpuCtx<'_>, images: &ImageRegistry) {
        // Destructured so the upload borrows `bgl`/`sampler` while the
        // closure holds `bindings` mutably — disjoint fields, which
        // `self.upload(..)` inside the closure could not express.
        let Self {
            bindings,
            bgl,
            sampler,
        } = self;
        images.drain_pending(|id, image| {
            let bind_group = upload(ctx.device, ctx.queue, bgl, sampler, id, &image);
            bindings.insert(id, bind_group);
        });
        images.drain_dropped(|id| {
            self.bindings.remove(&id);
        });
    }

    /// Bind `view` against the shared layout + sampler. The `GpuView`
    /// target allocator goes through here so target bind groups are
    /// built exactly like image ones — `draw` cannot tell them apart.
    pub(super) fn bind_group(
        &self,
        device: &wgpu::Device,
        view: &wgpu::TextureView,
        label: &str,
    ) -> wgpu::BindGroup {
        texture_bind_group(device, &self.bgl, &self.sampler, view, label)
    }
}

fn upload(
    device: &wgpu::Device,
    queue: &Queue,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    id: TextureId,
    image: &Image,
) -> wgpu::BindGroup {
    let raw_id = id.0;
    let size = wgpu::Extent3d {
        width: image.size.x,
        height: image.size.y,
        depth_or_array_layers: 1,
    };
    let texture_label = format!("palantir.image.tex.{raw_id:016x}");
    let bind_group_label = format!("palantir.image.tex.bg.{raw_id:016x}");
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(&texture_label),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &image.pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(image.size.x * 4),
            rows_per_image: Some(image.size.y),
        },
        size,
    );
    let view = texture.create_view(&Default::default());
    texture_bind_group(device, layout, sampler, &view, &bind_group_label)
}
