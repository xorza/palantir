use crate::gpu::device::gpu_wait::GpuWait;
use crate::gpu::surface::owned_target::OwnedTarget;
use crate::gpu::test_gpu::{TestGpu, headless_test_gpu, isolated_headless_test_gpu};
use crate::gpu::texel::TargetSpec;
use crate::internals::panic_probe;
use glam::UVec2;
use image::Rgba;
use std::iter;

/// A 3 × 2 texture cleared to (0.2, 0.4, 0.6, 1.0) reads back as six pixels of
/// (51, 102, 153, 255) — 0.2 · 255 = 51, 0.4 · 255 = 102, 0.6 · 255 = 153 — in both
/// channel orders. Three pixels make a 12-byte row, far below the 256-byte copy alignment,
/// so the padding is cut from every row.
#[test]
fn a_cleared_texture_reads_back_in_rgba_order() {
    let gpu = headless_test_gpu();
    for format in [
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Bgra8Unorm,
    ] {
        let target = OwnedTarget::new(
            &gpu,
            TargetSpec {
                label: "palantir.read_back.test",
                size: UVec2::new(3, 2),
                format,
                usage: OwnedTarget::USAGES,
            },
        );
        clear(&gpu, target.texture());
        let image = target.read_back();
        assert_eq!(image.dimensions(), (3, 2), "{format:?}");
        assert!(
            image
                .pixels()
                .all(|pixel| *pixel == Rgba([51, 102, 153, 255])),
            "{format:?}: {:?}",
            image.pixels().collect::<Vec<_>>()
        );
    }
}

/// A texel copy of the cleared 3 × 2 target takes its 24 bytes, the row padding cut, and a
/// second copy goes through the staging buffer the first made. On a lost device the copy fails
/// and leaves `texels` empty, and the buffer stays.
#[test]
fn a_texel_copy_keeps_its_staging_buffer() {
    let gpu = isolated_headless_test_gpu();
    let mut target = OwnedTarget::new(
        &gpu,
        TargetSpec {
            label: "palantir.copy_texels.test",
            size: UVec2::new(3, 2),
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: OwnedTarget::USAGES,
        },
    );
    clear(&gpu, target.texture());
    let mut texels = Vec::new();
    assert_eq!(target.copy_texels(&mut texels), GpuWait::Done);
    assert_eq!(texels, [51, 102, 153, 255].repeat(6));
    assert!(target.readback.is_some());
    texels.clear();
    assert_eq!(target.copy_texels(&mut texels), GpuWait::Done);
    assert_eq!(texels, [51, 102, 153, 255].repeat(6), "the kept buffer");

    gpu.lose();
    assert_eq!(target.copy_texels(&mut texels), GpuWait::DeviceLost);
    assert!(texels.is_empty());
    assert!(target.readback.is_some());
}

/// A zero side, or one past the device's largest 2D texture, panics before wgpu sees it; the
/// largest side itself is a target, and `resized` keeps the format and usages.
#[test]
fn a_bad_extent_panics_with_its_rule() {
    let gpu = headless_test_gpu();
    let handles = &*gpu;
    let max = handles.max_texture_dimension().get();
    let rule = format!("a target's size is 1..={max} physical pixels per side");
    let spec = |size| TargetSpec {
        label: "palantir.owned_target.extent",
        size,
        format: wgpu::TextureFormat::Bgra8UnormSrgb,
        usage: OwnedTarget::USAGES,
    };
    for size in [UVec2::new(0, 4), UVec2::new(4, 0), UVec2::new(max + 1, 1)] {
        panic_probe::assert_panics_with(&rule, || OwnedTarget::new(handles, spec(size)));
    }
    let widest = OwnedTarget::new(handles, spec(UVec2::new(max, 1)));
    assert_eq!(widest.size(), UVec2::new(max, 1));
    let small = widest.resized("palantir.owned_target.resized", UVec2::new(4, 2));
    assert_eq!(small.size(), UVec2::new(4, 2));
    assert_eq!(
        small.texture().format(),
        wgpu::TextureFormat::Bgra8UnormSrgb
    );
    assert_eq!(small.texture().usage(), OwnedTarget::USAGES);
}

fn clear(gpu: &TestGpu, texture: &wgpu::Texture) {
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("palantir.read_back.test.clear"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color {
                    r: 0.2,
                    g: 0.4,
                    b: 0.6,
                    a: 1.0,
                }),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    }));
    gpu.queue.submit(iter::once(encoder.finish()));
}
