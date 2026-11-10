//! Device-side gradient LUT atlas, owned by [`WgpuBackend`](crate::gpu::wgpu_backend::WgpuBackend),
//! whose group is the frame group every color pipeline reads at
//! [`FRAME_GROUP`](crate::primitives::wgsl::FRAME_GROUP).

use crate::gpu::device::gpu::Gpu;
use crate::gpu::frame::gpu_ctx::{GpuCtx, RowCopy};
use crate::gpu::surface::owned_target::TargetSpec;
use crate::gpu::texel;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::gradient_atlas::FlushedRows;
use crate::renderer::gradient_atlas::bake::LUT_ROW_TEXELS;
use glam::UVec2;
use std::ops::Range;

/// Bytes per uploaded LUT row, derived from the CPU `RgbaF16` row store.
const ROW_PITCH: u32 = (LUT_ROW_TEXELS * size_of::<RgbaF16>()) as u32;
// A buffer-to-texture copy reads rows at a 256-byte pitch, so packed rows stage in one run.
const _: () = assert!(
    ROW_PITCH.is_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
    "gradient atlas row pitch must stay 256-aligned to stage rows as packed"
);

/// The texture the frontend's gradient atlas uploads into, which the frame group binds.
/// Format-independent.
#[derive(Debug)]
pub(crate) struct GpuGradientAtlas {
    /// LUT atlas texture: 256 cols × N rows of `Rgba16Float` linear values (8-bit
    /// would band dark stops). [`Self::upload`] fills it and replaces it when the CPU atlas outgrew it.
    texture: wgpu::Texture,
    /// The view the frame group binds, made again with the texture.
    view: wgpu::TextureView,
    /// Counts the textures made, so the frame group sees a replaced one.
    generation: u64,
}

/// Allocates the LUT atlas texture at `rows` rows; shaders read the height via `textureDimensions`.
/// It gives copies, so the texture that replaces it takes the rows it holds by a copy.
fn create_texture(gpu: &Gpu, rows: u32) -> wgpu::Texture {
    texel::texture_2d(
        gpu,
        TargetSpec {
            label: "palantir.gradient_atlas",
            size: UVec2::new(LUT_ROW_TEXELS as u32, rows),
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
        },
    )
}

/// `runs`, each a range of `texture`'s rows whose bytes `bytes_of` gives, as one copy a run.
pub(super) fn row_copies<'a>(
    texture: &'a wgpu::Texture,
    runs: &'a [Range<u32>],
    bytes_of: impl Fn(Range<u32>) -> &'a [u8] + Clone,
) -> impl Iterator<Item = RowCopy<'a>> + Clone {
    runs.iter().map(move |run| RowCopy {
        texture,
        origin: UVec2::new(0, run.start),
        size: UVec2::new(LUT_ROW_TEXELS as u32, run.end - run.start),
        rows: bytes_of(run.clone()),
    })
}

impl GpuGradientAtlas {
    /// The texture at `rows` rows, the CPU atlas's height.
    pub(crate) fn new(gpu: &Gpu, rows: u32) -> Self {
        let texture = create_texture(gpu, rows);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture,
            view,
            generation: 0,
        }
    }

    /// The view the frame group binds.
    pub(crate) const fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// Which texture [`Self::view`] views: it changes where the atlas grows.
    pub(crate) const fn generation(&self) -> u64 {
        self.generation
    }

    /// Syncs the LUT atlas to the GPU: the dirty `rows` stage once in the belt and copy one per
    /// run ([`GpuCtx::copy_rows`]), on the frame's encoder ahead of every pass. A grown atlas
    /// replaces the texture and its view first, and the new texture takes the old one's rows
    /// by a copy on the GPU, as resident rows keep their ids. Called from `WgpuBackend::submit`,
    /// before the frame group binds the view.
    pub(crate) fn upload(&mut self, ctx: &mut GpuCtx<'_>, rows: FlushedRows<'_>) {
        if self.texture.height() != rows.total_rows {
            debug_assert!(
                self.texture.height() < rows.total_rows,
                "the atlas only grows"
            );
            let grown = create_texture(ctx.gpu, rows.total_rows);
            ctx.encoder.copy_texture_to_texture(
                self.texture.as_image_copy(),
                grown.as_image_copy(),
                self.texture.size(),
            );
            self.texture = grown;
            self.view = self
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            self.generation += 1;
        }
        ctx.copy_rows(row_copies(&self.texture, rows.runs, move |run| {
            rows.bytes(run)
        }));
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::resource::gpu_gradient_atlas::{GpuGradientAtlas, ROW_PITCH};
    use crate::gpu::test_gpu::{TestGpu, headless_test_gpu};
    use crate::primitives::paint::color::rgba_f16::RgbaF16;
    use crate::renderer::gradient_atlas::bake::{self, LUT_ROW_TEXELS};
    use crate::renderer::gradient_atlas::tests::support::distinct_grad;
    use crate::renderer::gradient_atlas::{CpuGradientAtlas, INITIAL_ATLAS_ROWS};

    /// Upload what `cpu` baked since its last flush into `atlas` on a frame's encoder of its own,
    /// and wait for it.
    fn upload(gpu: &TestGpu, cpu: &mut CpuGradientAtlas, atlas: &mut GpuGradientAtlas) {
        let rows = cpu.flush().expect("rows were baked");
        gpu.frame(|ctx| atlas.upload(ctx, rows));
    }

    /// A grown atlas takes the rows its old texture held by a copy on the GPU: the 255 rows the
    /// first frame uploaded read back as baked in the doubled texture, after a frame that
    /// re-registers them all and adds one, which the frame uploads alone.
    #[test]
    fn a_grown_atlas_copies_the_rows_it_held() {
        let gpu = headless_test_gpu();
        let mut cpu = CpuGradientAtlas::default();
        let mut atlas = GpuGradientAtlas::new(&gpu, cpu.capacity());
        let filled = INITIAL_ATLAS_ROWS - 1;
        let rows: Vec<u32> = (0..filled)
            .map(|i| cpu.register(&distinct_grad(i).ramp).0)
            .collect();
        upload(&gpu, &mut cpu, &mut atlas);
        for i in 0..filled {
            assert_eq!(
                cpu.register(&distinct_grad(i).ramp).0,
                rows[i as usize],
                "a resident row keeps its id"
            );
        }
        let added = cpu.register(&distinct_grad(filled).ramp).0;
        assert_eq!(cpu.capacity(), INITIAL_ATLAS_ROWS * 2);
        upload(&gpu, &mut cpu, &mut atlas);
        assert_eq!(atlas.texture.height(), INITIAL_ATLAS_ROWS * 2);

        let texels = gpu.read_texture(&atlas.texture);
        let row_texels = |row: u32| {
            let at = (row * ROW_PITCH) as usize;
            bytemuck::cast_slice::<u8, RgbaF16>(&texels[at..at + ROW_PITCH as usize]).to_vec()
        };
        let baked = |i: u32| {
            let mut out = [RgbaF16::TRANSPARENT; LUT_ROW_TEXELS];
            bake::row(&distinct_grad(i).ramp, &mut out);
            out.to_vec()
        };
        for i in 0..=filled {
            let row = if i < filled { rows[i as usize] } else { added };
            assert_eq!(row_texels(row), baked(i), "ramp {i} at row {row}");
        }
    }
}
