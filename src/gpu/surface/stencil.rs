//! The per-window stencil attachment and the stencil-test state shared by every
//! rounded-clip-aware pipeline. Masks stamp as a depth-counted stack
//! (`mask_stamp`: level `k` writes `k + 1` where the stencil equals `k`), and
//! color draws test at `stencil_reference = chain depth`.

use crate::gpu::surface::render_target;
use glam::UVec2;

use crate::renderer::render_buffer::MAX_ROUNDED_CLIP_DEPTH;

/// A window's stencil attachment for rounded-clip masking, sized to the render target; its
/// [`RenderStream`](crate::gpu::wgpu_backend::render_stream::RenderStream) makes and keeps it.
#[derive(Debug)]
pub(crate) struct Stencil {
    /// Held for its extent, which [`Self::fits`] compares before reuse.
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl Stencil {
    /// `Stencil8` suffices for the mask path; UI is 2D.
    pub(crate) const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Stencil8;

    /// Depth/stencil state for one rounded-clip pipeline; only `compare`,
    /// `pass_op` and `write_mask` differ, so the rest is stated once.
    fn state(
        compare: wgpu::CompareFunction,
        pass_op: wgpu::StencilOperation,
        write_mask: u32,
    ) -> wgpu::DepthStencilState {
        let face = wgpu::StencilFaceState {
            compare,
            fail_op: wgpu::StencilOperation::Keep,
            depth_fail_op: wgpu::StencilOperation::Keep,
            pass_op,
        };
        wgpu::DepthStencilState {
            format: Self::FORMAT,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: wgpu::StencilState {
                front: face,
                back: face,
                read_mask: MAX_ROUNDED_CLIP_DEPTH,
                write_mask,
            },
            bias: wgpu::DepthBiasState::default(),
        }
    }

    /// The stencil-test color pipelines: ref 0 outside masks, chain depth
    /// inside, compared `Equal`; `write_mask = 0` keeps the stamped masks.
    pub(crate) fn test_state() -> wgpu::DepthStencilState {
        Self::state(
            wgpu::CompareFunction::Equal,
            wgpu::StencilOperation::Keep,
            0x00,
        )
    }

    /// The mask-stamp variant, drawn per level at `stencil_reference = level`:
    /// writes `level + 1` where the SDF passes and the stencil equals `level`.
    pub(crate) fn stamp_state() -> wgpu::DepthStencilState {
        Self::state(
            wgpu::CompareFunction::Equal,
            wgpu::StencilOperation::IncrementClamp,
            MAX_ROUNDED_CLIP_DEPTH,
        )
    }

    /// The mask-clear variant, drawn at reference 0; the outermost quad suffices.
    pub(crate) fn clear_state() -> wgpu::DepthStencilState {
        Self::state(
            wgpu::CompareFunction::Always,
            wgpu::StencilOperation::Replace,
            MAX_ROUNDED_CLIP_DEPTH,
        )
    }

    /// The attachment view, behind an accessor so a `&Stencil` holder cannot reach the texture.
    pub(crate) const fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// Whether the attachment is `size` physical px.
    pub(crate) fn fits(&self, size: UVec2) -> bool {
        self.texture.size() == render_target::extent(size)
    }

    /// A stencil attachment of `size` physical px, its texels written to 0 by a copy before any
    /// pass: on NVIDIA under Vulkan, an `S8_UINT` image bound to memory that held nonzero bytes
    /// keeps 4 × 4 blocks that neither a load clear nor a stencil write resets, and every draw of a
    /// rounded-clip pass then fails there (`.notes/research/2026-10-31-shared-device-tile-loss.md`).
    pub(crate) fn new(device: &wgpu::Device, queue: &wgpu::Queue, size: UVec2) -> Self {
        let extent = render_target::extent(size);
        let usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST;
        #[cfg(test)]
        let usage = usage | wgpu::TextureUsages::COPY_SRC;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("palantir.renderer.stencil"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::FORMAT,
            usage,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::StencilOnly,
            },
            &vec![0; extent.width as usize * extent.height as usize],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(extent.width),
                rows_per_image: None,
            },
            extent,
        );
        Self {
            view: texture.create_view(&wgpu::TextureViewDescriptor::default()),
            texture,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::surface::stencil::Stencil;
    use crate::gpu::test_gpu::isolated_headless_test_gpu;
    use glam::UVec2;
    use std::iter;
    use std::sync::mpsc;

    /// A stencil made where freed buffers held nonzero bytes reads 0 in every texel after a pass
    /// that clears it, as each rounded-clip pass does. Each round frees the last round's stencil
    /// and 3 to 6 buffers of 256 to 16 640 bytes, each filled with one nonzero byte, so the
    /// allocator puts the next stencil over them. On an RTX 4090 Laptop GPU (NVIDIA 615.78.08)
    /// about 1 such stencil in 70 held 128, 130 or 132 in some 4 × 4 blocks without the zero
    /// write: 600 rounds miss it with odds of (69 / 70)^600, about 1 in 5600.
    #[test]
    fn a_stencil_on_memory_buffers_held_clears_to_zero() {
        const SIZE: UVec2 = UVec2::new(160, 120);
        const ROW: u32 = 256;
        let gpu = isolated_headless_test_gpu();
        let mut seed = 0x1234_5678_u32;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        let mut held: Option<(Stencil, Vec<wgpu::Buffer>)> = None;
        for round in 0..600 {
            drop(held.take());
            gpu.wait();
            let buffers: Vec<wgpu::Buffer> = (0..3 + next() % 4)
                .map(|_| {
                    let size = u64::from(256 + next() % 16384) / 16 * 16;
                    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("test.stencil.held"),
                        size,
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    let fill = u8::try_from(next() % 255 + 1).unwrap();
                    gpu.queue
                        .write_buffer(&buffer, 0, &vec![fill; usize::try_from(size).unwrap()]);
                    buffer
                })
                .collect();
            let stencil = Stencil::new(&gpu.device, &gpu.queue, SIZE);
            let mut encoder = gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("test.stencil.clear"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: stencil.view(),
                    depth_ops: None,
                    stencil_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0),
                        store: wgpu::StoreOp::Store,
                    }),
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            }));
            let read = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("test.stencil.read"),
                size: u64::from(ROW * SIZE.y),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &stencil.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::StencilOnly,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &read,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(ROW),
                        rows_per_image: None,
                    },
                },
                stencil.texture.size(),
            );
            gpu.queue.submit(iter::once(encoder.finish()));
            let slice = read.slice(..);
            let (tx, rx) = mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |mapped| {
                tx.send(mapped).ok();
            });
            gpu.wait();
            rx.recv().unwrap().unwrap();
            let texels = slice.get_mapped_range().unwrap();
            let spoiled = (0..SIZE.y)
                .flat_map(|y| (0..SIZE.x).map(move |x| (x, y)))
                .find(|&(x, y)| texels[(y * ROW + x) as usize] != 0);
            assert_eq!(spoiled, None, "round {round}");
            held = Some((stencil, buffers));
        }
    }
}
