use crate::gpu::resource::single_quad_buffer::SingleQuadBuffer;
use crate::gpu::test_gpu::headless_test_gpu;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::paint::color::RgbaF32;
use crate::renderer::quad::Quad;

/// A quad equal to the held one is not sent again; any change is,
/// and a return to an earlier quad is a change too.
#[test]
fn writes_only_a_changed_quad() {
    let gpu = headless_test_gpu();
    let mut buffer = SingleQuadBuffer::new(&gpu, "test.single_quad");
    let quad = |w: f32, alpha: f32| Quad {
        rect: Rect::new(0.0, 0.0, w, 100.0),
        fill: RgbaF32::new(0.0, 0.0, 0.0, alpha).to_f16(),
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
    gpu.frame(|ctx| {
        for (at, (q, wrote)) in writes.into_iter().enumerate() {
            assert_eq!(buffer.upload(ctx, q), wrote, "upload {at}");
        }
    });
    assert!(buffer.is_uploaded());
}
