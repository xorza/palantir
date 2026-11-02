use crate::gpu::surface::stencil::Stencil;
use crate::gpu::test_gpu::isolated_headless_test_gpu;
use glam::UVec2;
use std::iter;

/// A stencil made where freed buffers held nonzero bytes reads 0 in every texel after a pass
/// that clears it, as each rounded-clip pass does. Each round frees the last round's stencil
/// and 3 to 6 buffers of 256 to 16 640 bytes, each filled with one nonzero byte, so the
/// allocator puts the next stencil over them. On an high-end NVIDIA GPU GPU (NVIDIA 615.78.08)
/// about 1 such stencil in 70 held 128, 130 or 132 in some 4 × 4 blocks without the zero
/// write: 600 rounds miss it with odds of (69 / 70)^600, about 1 in 5600.
#[test]
fn a_stencil_on_memory_buffers_held_clears_to_zero() {
    const SIZE: UVec2 = UVec2::new(160, 120);
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
        let stencil = Stencil::new(&gpu, SIZE);
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
        gpu.queue.submit(iter::once(encoder.finish()));
        let texels = gpu.read_texture(&stencil.texture);
        let spoiled = (0..SIZE.y)
            .flat_map(|y| (0..SIZE.x).map(move |x| (x, y)))
            .find(|&(x, y)| texels[(y * SIZE.x + x) as usize] != 0);
        assert_eq!(spoiled, None, "round {round}");
        held = Some((stencil, buffers));
    }
}
