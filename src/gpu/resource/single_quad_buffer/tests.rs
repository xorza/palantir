use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::gpu::resource::single_quad_buffer::SingleQuadBuffer;
use crate::gpu::test_gpu::headless_test_gpu;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::paint::color::RgbaF32;
use crate::renderer::quad::Quad;
use wgpu::util::StagingBelt;

/// A quad equal to the held one is not sent again; any change is,
/// and a return to an earlier quad is a change too.
#[test]
fn writes_only_a_changed_quad() {
    let gpu = headless_test_gpu();
    let device = &gpu.device;
    let mut buffer = SingleQuadBuffer::new(device, "test.single_quad");
    let mut belt = StagingBelt::new(device.clone(), 1 << 12);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    let mut ctx = GpuCtx::new(device, &gpu.queue, &mut belt, &mut encoder);
    let quad = |w: f32, alpha: f32| Quad {
        rect: Rect::new(0.0, 0.0, w, 100.0),
        fill: RgbaF32::new(0.0, 0.0, 0.0, alpha).into(),
        ..Default::default()
    };

    assert!(!buffer.is_uploaded());
    let writes = [
        (quad(200.0, 0.5), true),
        (quad(200.0, 0.5), false),
        (quad(300.0, 0.5), true),
        (quad(300.0, 1.0), true),
        (quad(200.0, 0.5), true),
        (quad(200.0, 0.5), false),
    ];
    for (at, (q, wrote)) in writes.into_iter().enumerate() {
        assert_eq!(buffer.upload(&mut ctx, q), wrote, "upload {at}");
    }
    assert!(buffer.is_uploaded());
}
