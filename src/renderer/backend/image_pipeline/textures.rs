//! Registered-image GPU bindings and their upload/drop lifecycle.

use crate::primitives::image::Image;
use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::pipeline_utils::texture_bind_group;
use crate::renderer::backend::queue::Queue;
use crate::renderer::image_registry::ImageRegistry;
use crate::renderer::texture_id::TextureId;
use rustc_hash::FxHashMap;

#[derive(Debug, Default)]
pub(crate) struct ImageTextures {
    pub(crate) bindings: FxHashMap<TextureId, wgpu::BindGroup>,
}

impl ImageTextures {
    #[profiling::function]
    pub(crate) fn drain_registry(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        images: &ImageRegistry,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
    ) {
        images.drain_pending(|id, image| {
            let bind_group = upload(ctx.device, ctx.queue, layout, sampler, id, &image);
            self.bindings.insert(id, bind_group);
        });
        images.drain_dropped(|id| {
            self.bindings.remove(&id);
        });
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
    assert_within_device_limit(image.size, device.limits().max_texture_dimension_2d);
    let raw_id = id.0;
    let size = wgpu::Extent3d {
        width: image.size.x,
        height: image.size.y,
        depth_or_array_layers: 1,
    };
    let texture_label = format!("aperture.image.tex.{raw_id:016x}");
    let bind_group_label = format!("aperture.image.tex.bg.{raw_id:016x}");
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

fn assert_within_device_limit(size: glam::UVec2, max_dim: u32) {
    assert!(
        size.x <= max_dim && size.y <= max_dim,
        "registered image is {}x{} px but the device's \
         max_texture_dimension_2d is {max_dim}; downscale or tile it \
         before Ui::register_image",
        size.x,
        size.y,
    );
}

#[cfg(test)]
mod tests {
    use crate::renderer::backend::image_pipeline::textures::assert_within_device_limit;
    use glam::UVec2;

    #[test]
    fn device_limit_boundaries_are_accepted() {
        assert_within_device_limit(UVec2::ONE, 8192);
        assert_within_device_limit(UVec2::splat(8192), 8192);
    }

    #[test]
    #[should_panic(expected = "max_texture_dimension_2d is 8192")]
    fn oversized_image_names_device_limit() {
        assert_within_device_limit(UVec2::new(8193, 4), 8192);
    }
}
