//! Device-side gradient LUT atlas, owned by [`WgpuBackend`](crate::gpu::wgpu_backend::WgpuBackend)
//! and lent to the quad and curve pipelines.

use crate::gpu::device::gpu_ctx::{GpuCtx, Staged};
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::gradient_atlas::bake::LUT_ROW_TEXELS;
use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;

/// Bytes per uploaded LUT row, derived from the CPU `RgbaF16` row store.
const ROW_PITCH: u32 = (LUT_ROW_TEXELS * size_of::<RgbaF16>()) as u32;
// A buffer-to-texture copy reads rows at a 256-byte pitch, so packed rows stage as they are.
const _: () = assert!(
    ROW_PITCH.is_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
    "gradient atlas row pitch must stay 256-aligned to stage rows as packed"
);

/// Shared CPU gradient source plus the texture and bind group both pipelines consume. Format-independent.
#[derive(Debug)]
pub(crate) struct GpuGradientAtlas {
    cpu: SharedGradientAtlas,
    /// LUT atlas texture: 256 cols × N rows of `Rgba16Float` linear values (8-bit
    /// would band dark stops). [`Self::upload`] fills it and replaces it when the CPU atlas outgrew it.
    texture: wgpu::Texture,
    /// The shared sampled-texture binding, kept to rebuild [`Self::bind_group`]; height-independent.
    binding: TextureBinding,
    /// Group-0 bind group for both pipelines.
    pub(crate) bind_group: wgpu::BindGroup,
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
        let bind_group = binding.bind_group(device, &view, "palantir.gradient.bg");

        Self {
            cpu,
            texture,
            binding,
            bind_group,
        }
    }

    /// Syncs the LUT atlas to the GPU if anything changed: the dirty runs stage once in the
    /// belt and copy one per run, on the frame's encoder ahead of every pass. A grown atlas
    /// replaces the texture and bind group first, and growth dirties every row so the same
    /// upload refills it. Called from `WgpuBackend::submit`.
    pub(crate) fn upload(&mut self, ctx: &mut GpuCtx<'_>) {
        // Destructured so the resize borrows the GPU fields while `flush_with` holds the CPU atlas.
        let Self {
            cpu,
            texture,
            binding,
            bind_group,
        } = self;
        cpu.flush_with(|rows| {
            if texture.height() != rows.total_rows {
                *texture = create_texture(ctx.device, rows.total_rows);
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                *bind_group = binding.bind_group(ctx.device, &view, "palantir.gradient.bg");
            }
            let Staged {
                mut view,
                buffer,
                offset,
                encoder,
            } = ctx
                .stage(
                    rows.len_bytes() as u64,
                    u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
                )
                .expect("a flush holds at least one row");
            let mut at = 0;
            for run in rows.runs {
                let bytes = rows.bytes(run.clone());
                view.slice(at..at + bytes.len()).copy_from_slice(bytes);
                encoder.copy_buffer_to_texture(
                    wgpu::TexelCopyBufferInfo {
                        buffer,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: offset + at as u64,
                            bytes_per_row: Some(ROW_PITCH),
                            rows_per_image: None,
                        },
                    },
                    wgpu::TexelCopyTextureInfo {
                        texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: 0,
                            y: run.start,
                            z: 0,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::Extent3d {
                        width: LUT_ROW_TEXELS as u32,
                        height: run.end - run.start,
                        depth_or_array_layers: 1,
                    },
                );
                at += bytes.len();
            }
        });
    }
}
