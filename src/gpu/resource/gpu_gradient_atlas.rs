//! Device-side gradient LUT atlas, owned by [`WgpuBackend`](crate::gpu::wgpu_backend::WgpuBackend)
//! and lent to the quad and curve pipelines.

use crate::gpu::device::gpu_ctx::{GpuCtx, Staged};
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::surface::render_target;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::gradient_atlas::bake::LUT_ROW_TEXELS;
use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
use glam::UVec2;

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
/// It gives copies, so the texture that replaces it takes the rows it holds by a copy.
fn create_texture(device: &wgpu::Device, rows: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("palantir.gradient_atlas"),
        size: render_target::extent(UVec2::new(LUT_ROW_TEXELS as u32, rows)),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
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
    /// replaces the texture and bind group first, and the new texture takes the old one's rows
    /// by a copy on the GPU, as resident rows keep their ids. Called from `WgpuBackend::submit`.
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
                debug_assert!(texture.height() < rows.total_rows, "the atlas only grows");
                let grown = create_texture(ctx.device, rows.total_rows);
                ctx.encoder.copy_texture_to_texture(
                    texture.as_image_copy(),
                    grown.as_image_copy(),
                    texture.size(),
                );
                *texture = grown;
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                *bind_group = binding.bind_group(ctx.device, &view, "palantir.gradient.bg");
            }
            // A flush holds at least one row, so only a lost device declines the staging.
            let Some(Staged {
                mut view,
                buffer,
                offset,
                encoder,
            }) = ctx.stage(
                rows.len_bytes() as u64,
                u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
            )
            else {
                return;
            };
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
                    render_target::extent(UVec2::new(LUT_ROW_TEXELS as u32, run.end - run.start)),
                );
                at += bytes.len();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::device::gpu_ctx::GpuCtx;
    use crate::gpu::resource::gpu_gradient_atlas::{GpuGradientAtlas, ROW_PITCH};
    use crate::gpu::resource::texture_binding::TextureBinding;
    use crate::gpu::test_gpu::{HeadlessTestGpuLease, headless_test_gpu};
    use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
    use crate::primitives::paint::brush::gradient::linear_geometry::LinearGradient;
    use crate::primitives::paint::color::RgbaF32;
    use crate::primitives::paint::color::rgba_f16::RgbaF16;
    use crate::primitives::paint::color::srgba_u8::SrgbaU8;
    use crate::renderer::gradient_atlas::INITIAL_ATLAS_ROWS;
    use crate::renderer::gradient_atlas::bake::{self, LUT_ROW_TEXELS};
    use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
    use std::iter;
    use wgpu::util::StagingBelt;

    /// A ramp of its own for every `i` below 2^24.
    fn ramp(i: u32) -> ColorRamp {
        let [r, g, b, _] = i.to_le_bytes();
        LinearGradient::two_stop(0.0, SrgbaU8::rgb(r, g, b).into(), RgbaF32::WHITE).ramp
    }

    /// Upload `atlas` on a frame's encoder of its own, and wait for it.
    fn upload(gpu: &HeadlessTestGpuLease, atlas: &mut GpuGradientAtlas) {
        let mut belt = StagingBelt::new(gpu.device.clone(), 1 << 16);
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        atlas.upload(&mut GpuCtx::new(gpu, &mut belt, &mut encoder));
        belt.finish();
        gpu.queue.submit(iter::once(encoder.finish()));
        gpu.wait();
    }

    /// A grown atlas takes the rows its old texture held by a copy on the GPU: the 255 rows the
    /// first frame uploaded read back as baked in the doubled texture, after a frame that
    /// re-registers them all and adds one, which the frame uploads alone.
    #[test]
    fn a_grown_atlas_copies_the_rows_it_held() {
        let gpu = headless_test_gpu();
        let cpu = SharedGradientAtlas::default();
        let mut atlas =
            GpuGradientAtlas::new(&gpu.device, cpu.clone(), TextureBinding::new(&gpu.device));
        let filled = INITIAL_ATLAS_ROWS - 1;
        let rows: Vec<u32> = (0..filled).map(|i| cpu.register(&ramp(i)).0).collect();
        upload(&gpu, &mut atlas);
        for i in 0..filled {
            assert_eq!(
                cpu.register(&ramp(i)).0,
                rows[i as usize],
                "a resident row keeps its id"
            );
        }
        let added = cpu.register(&ramp(filled)).0;
        assert_eq!(cpu.rows(), INITIAL_ATLAS_ROWS * 2);
        upload(&gpu, &mut atlas);
        assert_eq!(atlas.texture.height(), INITIAL_ATLAS_ROWS * 2);

        let texels = gpu.read_texture(&atlas.texture);
        let row_texels = |row: u32| {
            let at = (row * ROW_PITCH) as usize;
            bytemuck::cast_slice::<u8, RgbaF16>(&texels[at..at + ROW_PITCH as usize]).to_vec()
        };
        let baked = |i: u32| {
            let mut out = [RgbaF16::TRANSPARENT; LUT_ROW_TEXELS];
            bake::row(&ramp(i), &mut out);
            out.to_vec()
        };
        for i in 0..=filled {
            let row = if i < filled { rows[i as usize] } else { added };
            assert_eq!(row_texels(row), baked(i), "ramp {i} at row {row}");
        }
    }
}
