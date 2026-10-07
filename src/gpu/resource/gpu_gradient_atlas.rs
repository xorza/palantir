//! Device-side gradient LUT atlas, owned by [`WgpuBackend`](crate::gpu::wgpu_backend::WgpuBackend)
//! and lent to the quad and curve pipelines.

use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::resource::texture_region::TextureRegion;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::gradient_atlas::bake::LUT_ROW_TEXELS;
use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
use glam::UVec2;

/// Bytes per uploaded LUT row, derived from the CPU `RgbaF16` row store.
const ROW_PITCH: u32 = (LUT_ROW_TEXELS * size_of::<RgbaF16>()) as u32;
// A multiple of `COPY_BYTES_PER_ROW_ALIGNMENT` (256), which `write_texture`
// does not require but wgpu passes through in one copy; any other pitch is
// re-packed row by row.
const _: () = assert!(
    ROW_PITCH.is_multiple_of(256),
    "gradient atlas row pitch must stay 256-aligned to keep write_texture on its single-copy path"
);

/// Shared CPU gradient source plus the texture and bind group both pipelines consume. Format-independent.
#[derive(Debug)]
pub(crate) struct GpuGradientAtlas {
    cpu: SharedGradientAtlas,
    /// LUT atlas texture: 256 cols × N rows of `Rgba16Float` linear values (8-bit
    /// would band dark stops). [`Self::upload`] fills it and replaces it when the CPU atlas outgrew it.
    texture: wgpu::Texture,
    /// The shared sampled-texture binding, kept to rebuild [`Self::bg`]; height-independent.
    binding: TextureBinding,
    /// Group-0 bind group for both pipelines.
    pub(crate) bg: wgpu::BindGroup,
}

/// Allocates the LUT atlas texture at `rows` rows; shaders read the height via `textureDimensions`.
fn create_texture(device: &wgpu::Device, rows: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("palantir.gradient_atlas"),
        size: wgpu::Extent3d {
            width: LUT_ROW_TEXELS as u32,
            height: rows,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

impl GpuGradientAtlas {
    pub(crate) fn new(
        device: &wgpu::Device,
        cpu: SharedGradientAtlas,
        binding: TextureBinding,
    ) -> Self {
        let texture = create_texture(device, cpu.rows());
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bg = binding.bind_group(device, &view, "palantir.gradient.bg");

        Self {
            cpu,
            texture,
            binding,
            bg,
        }
    }

    /// Syncs the LUT atlas to the GPU if anything changed. A dirty frame
    /// uploads the dirty row span in one `write_texture`. A grown atlas
    /// replaces the texture and bind group first, and growth dirties every
    /// row so the same upload refills it. Called from `WgpuBackend::submit`.
    pub(crate) fn upload(&mut self, ctx: &GpuCtx<'_>) {
        // Destructured so the resize borrows the GPU fields while `flush_with` holds the CPU atlas.
        let Self {
            cpu,
            texture,
            binding,
            bg,
        } = self;
        cpu.flush_with(|rows| {
            if texture.height() != rows.total_rows {
                *texture = create_texture(ctx.device, rows.total_rows);
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                *bg = binding.bind_group(ctx.device, &view, "palantir.gradient.bg");
            }
            let height = rows.bytes.len() as u32 / ROW_PITCH;
            TextureRegion {
                texture,
                first_row: rows.first_row,
                size: UVec2::new(LUT_ROW_TEXELS as u32, height),
                bytes_per_row: ROW_PITCH,
            }
            .write(ctx.queue, rows.bytes);
        });
    }
}
