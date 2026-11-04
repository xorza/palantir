use crate::gpu::pipeline::clip_stencil;
use crate::gpu::pipeline::pipeline_recipe::{self, PipelineRecipe};
use crate::gpu::surface::owned_target::TargetSpec;
use crate::gpu::surface::stencil::Stencil;
use crate::gpu::test_gpu::isolated_headless_test_gpu;
use crate::gpu::texel;
use glam::UVec2;
use std::iter;

/// A full-target triangle that writes white where the stencil passes a rounded-clip test at
/// reference 0, as a color draw outside every clip does.
const PROBE: &str = "
@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    let uv = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4f(uv * 2.0 - 1.0, 0.0, 1.0);
}
@fragment fn fs() -> @location(0) vec4f {
    return vec4f(1.0);
}
";

/// A stencil made where freed buffers held nonzero bytes holds 0 in every texel after a pass
/// that clears it, as each rounded-clip pass does: a draw tested `Equal 0` there, as production
/// builds the stencil and tests it, writes every pixel, read back from a color target, so the
/// stencil is made with production's usage alone. Each round frees the last round's stencil
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
    let shader = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("test.stencil.probe"),
            source: wgpu::ShaderSource::Wgsl(PROBE.into()),
        });
    let layout = pipeline_recipe::pipeline_layout(&gpu, "test.stencil.probe.pl", &[]);
    let probe = PipelineRecipe {
        blend: None,
        depth_stencil: Some(clip_stencil::test_state()),
        ..PipelineRecipe::color(
            "test.stencil.probe",
            &shader,
            &layout,
            wgpu::TextureFormat::Rgba8Unorm,
        )
    }
    .build(&gpu)
    .pipeline;
    let color = texel::texture_2d(
        &gpu,
        TargetSpec {
            label: "test.stencil.color",
            size: SIZE,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        },
    );
    let color_view = color.create_view(&wgpu::TextureViewDescriptor::default());
    // Clear `stencil` to `cleared` in a pass, draw the probe tested `Equal 0`, and read the
    // color back: white where the stencil holds 0, black elsewhere.
    let probe_after_clear = |stencil: &Stencil, cleared: u32| {
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("test.stencil.clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: stencil.view(),
                    depth_ops: None,
                    stencil_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(cleared),
                        store: wgpu::StoreOp::Store,
                    }),
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&probe);
            pass.set_stencil_reference(0);
            pass.draw(0..3, 0..1);
        }
        gpu.queue.submit(iter::once(encoder.finish()));
        gpu.read_texture(&color)
    };
    // The probe sees a stencil that is not 0: cleared to 1, every pixel stays black.
    let ones = probe_after_clear(&Stencil::new(&gpu, SIZE), 1);
    assert!(
        ones.as_chunks::<4>()
            .0
            .iter()
            .all(|texel| *texel == [0, 0, 0, 255]),
        "the probe reads the stencil"
    );
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
        let texels = probe_after_clear(&stencil, 0);
        let spoiled = (0..SIZE.y)
            .flat_map(|y| (0..SIZE.x).map(move |x| (x, y)))
            .find(|&(x, y)| {
                let at = 4 * (y * SIZE.x + x) as usize;
                texels[at..at + 4] != [255; 4]
            });
        assert_eq!(spoiled, None, "round {round}");
        held = Some((stencil, buffers));
    }
}
