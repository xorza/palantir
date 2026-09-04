//! The GPU side of registered images: their textures and bind groups,
//! created, rewritten and freed the moment the registry asks.

use crate::primitives::texture_id::TextureId;
use crate::renderer::backend::image_binding::ImageBinding;
use crate::renderer::backend::texture_region::TextureRegion;
use glam::UVec2;
use rustc_hash::FxHashMap;

/// Every registered image's texture and the bind group a draw samples it
/// through, owned by the image registry once a backend attaches this.
///
/// Immediate on every operation. A registration creates and fills the
/// texture before it returns. A write lands through `write_texture`, which
/// copies into wgpu's staging at once. A dropped handle frees its texture,
/// which wgpu destroys once the GPU is done with it. The queue orders each
/// of them before the frame's draw, so nothing here waits for a frame
/// boundary.
#[derive(Debug)]
pub(crate) struct ImageGpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    binding: ImageBinding,
    textures: FxHashMap<TextureId, ImageTexture>,
}

#[derive(Debug)]
struct ImageTexture {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

impl ImageGpu {
    pub(crate) fn new(device: wgpu::Device, queue: wgpu::Queue, binding: ImageBinding) -> Self {
        Self {
            device,
            queue,
            binding,
            textures: FxHashMap::default(),
        }
    }

    /// Create the texture for `id` and fill it with `texels`, rows of
    /// `size.x * 4` bytes.
    pub(crate) fn create(&mut self, id: TextureId, size: UVec2, texels: &[u8]) {
        let raw_id = id.0;
        let texture_label = format!("palantir.image.tex.{raw_id:016x}");
        let bind_group_label = format!("palantir.image.tex.bg.{raw_id:016x}");
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&texture_label),
            size: wgpu::Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        write_all(&self.queue, &texture, size, texels);
        let view = texture.create_view(&Default::default());
        let bind_group = self
            .binding
            .bind_group(&self.device, &view, &bind_group_label);
        let replaced = self.textures.insert(
            id,
            ImageTexture {
                texture,
                bind_group,
            },
        );
        debug_assert!(replaced.is_none(), "an id is registered once");
    }

    /// Overwrite every texel of `id`'s texture.
    pub(crate) fn write(&self, id: TextureId, texels: &[u8]) {
        let texture = &self.textures[&id].texture;
        let size = UVec2::new(texture.width(), texture.height());
        write_all(&self.queue, texture, size, texels);
    }

    /// Free `id`'s texture and bind group.
    pub(crate) fn free(&mut self, id: TextureId) {
        self.textures.remove(&id);
    }

    /// What a draw binds for `id`, or `None` when no registered image
    /// answers to it.
    pub(super) fn bind_group(&self, id: TextureId) -> Option<&wgpu::BindGroup> {
        self.textures.get(&id).map(|entry| &entry.bind_group)
    }
}

fn write_all(queue: &wgpu::Queue, texture: &wgpu::Texture, size: UVec2, texels: &[u8]) {
    TextureRegion {
        texture,
        first_row: 0,
        size,
        bytes_per_row: size.x * 4,
    }
    .write(queue, texels);
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
    use crate::renderer::backend::image_gpu::ImageGpu;

    impl ImageGpu {
        /// Registered images resident on the GPU. The surface-format-change
        /// tests assert this survives a pipeline rebuild.
        pub(crate) fn resident(&self) -> usize {
            self.textures.len()
        }
    }
}
