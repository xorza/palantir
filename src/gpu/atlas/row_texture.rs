//! [`RowTexture`]: the texture of the pool's gradient rows, owned by
//! [`WgpuBackend`](crate::gpu::wgpu_backend::WgpuBackend), whose group is the frame group every
//! color pipeline reads at [`FRAME_GROUP`](crate::primitives::wgsl::FRAME_GROUP): rows, not atlas
//! items, as every color pipeline reads them branch-free (`src/gpu/AGENTS.md`, Atlases). Its CPU
//! half is the pool's [`RowTable`](crate::renderer::row_table::RowTable).

use crate::gpu::device::gpu::Gpu;
use crate::gpu::resource::frame_group::RowView;
use crate::gpu::resource::gpu_ctx::{GpuCtx, Staged};
use crate::gpu::texel;
use crate::gpu::texel::TargetSpec;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::row_table::FlushedRows;
use crate::renderer::row_table::bake::LUT_ROW_TEXELS;
use glam::UVec2;

/// Bytes per uploaded LUT row, derived from the CPU `RgbaF16` row store.
const ROW_PITCH: u32 = (LUT_ROW_TEXELS * size_of::<RgbaF16>()) as u32;
// A buffer-to-texture copy reads rows at a 256-byte pitch, so packed rows stage in one run.
const _: () = assert!(
    ROW_PITCH.is_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
    "a gradient row's pitch must stay 256-aligned to stage rows as packed"
);

/// The texture the pool's row table uploads into, which the frame group binds.
/// Format-independent.
#[derive(Debug)]
pub(crate) struct RowTexture {
    /// 256 cols × N rows of `Rgba16Float` linear values (8-bit would band dark stops).
    /// [`Self::upload`] fills it and replaces it when the table grew or shrank.
    texture: wgpu::Texture,
    /// The view the frame group binds, made again with the texture.
    view: wgpu::TextureView,
    /// Counts the textures made, so the frame group sees a replaced one.
    generation: u64,
}

/// Allocates the row texture at `rows` rows; shaders read the height via `textureDimensions`.
/// It gives copies, so the texture that replaces it takes the rows it holds by a copy.
fn create_texture(gpu: &Gpu, rows: u32) -> wgpu::Texture {
    texel::texture_2d(
        gpu,
        TargetSpec {
            label: "palantir.atlas.rows",
            size: UVec2::new(LUT_ROW_TEXELS as u32, rows),
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
        },
    )
}

impl RowTexture {
    /// The texture at `rows` rows, the table's height.
    pub(crate) fn new(gpu: &Gpu, rows: u32) -> Self {
        let texture = create_texture(gpu, rows);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture,
            view,
            generation: 0,
        }
    }

    /// The view the frame group binds, and which texture it views, which changes where the table
    /// grows or shrinks.
    pub(crate) const fn row_view(&self) -> RowView<'_> {
        RowView {
            view: &self.view,
            generation: self.generation,
        }
    }

    /// Follow the table to the GPU, on the frame's encoder ahead of every pass: a table of another
    /// height replaces the texture and its view first, the new one taking the rows both hold by a
    /// copy on the GPU, as the held rows keep their ids; then each dirty run's rows, baked into a
    /// row on the stack and written once into the belt, copy in one copy a run. A grow and a shrink
    /// take the one path, also on a frame with no dirty row. Called from `WgpuBackend::submit`
    /// before the frame group binds the view, and after a purge. A lost device declines the
    /// staging, and then nothing is copied.
    pub(crate) fn upload(&mut self, ctx: &mut GpuCtx<'_>, rows: FlushedRows<'_>) {
        let height = self.texture.height();
        if height != rows.capacity {
            let made = create_texture(ctx.gpu, rows.capacity);
            ctx.encoder.copy_texture_to_texture(
                self.texture.as_image_copy(),
                made.as_image_copy(),
                wgpu::Extent3d {
                    width: LUT_ROW_TEXELS as u32,
                    height: height.min(rows.capacity),
                    depth_or_array_layers: 1,
                },
            );
            self.texture = made;
            self.view = self
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            self.generation += 1;
        }
        let staged: u32 = rows.runs().iter().map(|run| run.end - run.start).sum();
        let Some(Staged {
            mut view,
            buffer,
            offset,
            encoder,
        }) = ctx.stage(
            u64::from(staged) * u64::from(ROW_PITCH),
            u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
        )
        else {
            return;
        };
        let mut texels = [RgbaF16::TRANSPARENT; LUT_ROW_TEXELS];
        let mut at = 0;
        for run in rows.runs() {
            let start = at;
            for id in run.clone() {
                rows.bake(id, &mut texels);
                view.slice(at..at + ROW_PITCH as usize)
                    .copy_from_slice(bytemuck::cast_slice(&texels));
                at += ROW_PITCH as usize;
            }
            encoder.copy_buffer_to_texture(
                wgpu::TexelCopyBufferInfo {
                    buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: offset + start as u64,
                        bytes_per_row: Some(ROW_PITCH),
                        rows_per_image: Some(run.end - run.start),
                    },
                },
                wgpu::TexelCopyTextureInfo {
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: run.start,
                        z: 0,
                    },
                    ..self.texture.as_image_copy()
                },
                wgpu::Extent3d {
                    width: LUT_ROW_TEXELS as u32,
                    height: run.end - run.start,
                    depth_or_array_layers: 1,
                },
            );
        }
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::gpu::atlas::row_texture::{ROW_PITCH, RowTexture};
    use crate::gpu::atlas_memory::{self, AtlasUse};

    impl RowTexture {
        /// The texture's memory against `live_rows`, the rows the table holds a ramp in.
        pub(crate) fn memory(&self, live_rows: u32) -> AtlasUse {
            AtlasUse {
                name: "gradient rows",
                textures: 1,
                bytes: atlas_memory::texture_bytes(&self.texture),
                live_bytes: Some(u64::from(live_rows) * u64::from(ROW_PITCH)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::common::expiry_window::EXPIRY_FRAMES;
    use crate::gpu::atlas::row_texture::{ROW_PITCH, RowTexture};
    use crate::gpu::test_gpu::{TestGpu, headless_test_gpu};
    use crate::primitives::paint::color::rgba_f16::RgbaF16;
    use crate::renderer::row_table::RowTable;
    use crate::renderer::row_table::bake::LutRowTexels;
    use crate::renderer::row_table::tests::support::{baked, magenta, ramp, table};

    /// Upload what `table` owes into `texture` on a frame's encoder of its own, wait for it, and
    /// read back each row.
    fn upload_and_read(
        gpu: &TestGpu,
        table: &mut RowTable,
        texture: &mut RowTexture,
    ) -> Vec<LutRowTexels> {
        gpu.frame(|ctx| texture.upload(ctx, table.flush()));
        assert_eq!(texture.texture.height(), table.capacity());
        let texels = gpu.read_texture(&texture.texture);
        let (rows, rest) = texels.as_chunks::<{ ROW_PITCH as usize }>();
        assert!(rest.is_empty(), "the texture reads back whole rows");
        rows.iter()
            .map(|row| {
                let mut out = [RgbaF16::TRANSPARENT; 256];
                out.copy_from_slice(bytemuck::cast_slice(row));
                out
            })
            .collect()
    }

    /// The texture follows the table by one path. Ramps 0 to 2 fill a table of 4 rows; ramps 3 to
    /// 5 in the same frame grow it to 8, and the texture made again takes rows 0 to 3 by a copy
    /// and the new rows by their bakes. Frames that draw ramp 5 alone, to an expiry window on, let
    /// the decay take the other five, ramp 5 moving to row 1, and drop the capacity to 4: the
    /// texture made again keeps row 0's magenta by the copy, as no flush owes row 0 again. A frame
    /// that adds ramps 6 and 7, then a purge in one that draws ramp 7 alone, moves ramp 7 to row 1.
    #[test]
    fn the_texture_follows_a_grow_a_shrink_and_a_purge() {
        let gpu = headless_test_gpu();
        let mut table = table(4, 4, 16);
        let mut texture = RowTexture::new(&gpu, table.capacity());
        let ramps = |table: &mut RowTable, ramps: std::ops::Range<u32>| {
            for i in ramps {
                table.register(&ramp(i));
            }
        };
        ramps(&mut table, 0..3);
        let read = upload_and_read(&gpu, &mut table, &mut texture);
        assert_eq!(read.len(), 4);
        ramps(&mut table, 3..6);
        let generation = texture.generation;
        let read = upload_and_read(&gpu, &mut table, &mut texture);
        assert_eq!(texture.generation, generation + 1);
        let want: Vec<LutRowTexels> = [[magenta(); 256]]
            .into_iter()
            .chain((0..6).map(|i| baked(&ramp(i))))
            .collect();
        assert_eq!(read[..7], want, "grown");

        for frame in 1..=EXPIRY_FRAMES + 1 {
            table.advance_to(frame);
            ramps(&mut table, 5..6);
            table.decay();
        }
        assert_eq!(table.capacity(), 4);
        let read = upload_and_read(&gpu, &mut table, &mut texture);
        assert_eq!(read[..2], [[magenta(); 256], baked(&ramp(5))], "shrunk");

        table.advance_to(EXPIRY_FRAMES + 5);
        ramps(&mut table, 6..8);
        upload_and_read(&gpu, &mut table, &mut texture);
        table.advance_to(EXPIRY_FRAMES + 6);
        ramps(&mut table, 7..8);
        table.purge();
        let read = upload_and_read(&gpu, &mut table, &mut texture);
        assert_eq!(read[..2], [[magenta(); 256], baked(&ramp(7))], "purged");
    }
}
