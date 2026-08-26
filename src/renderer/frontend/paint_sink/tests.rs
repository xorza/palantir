use crate::primitives::color::{Color, ColorF16};
use crate::primitives::rect::Rect;
use crate::primitives::texture_id::TextureId;
use crate::renderer::frontend::capture::{PaintCall, PaintCapture};
use crate::renderer::frontend::paint_sink::PaintGate;
use crate::renderer::frontend::payload::draw_image_payload::DrawImagePayload;
use crate::renderer::frontend::payload::draw_polyline_payload::DrawPolylinePayload;
use crate::renderer::gpu_paint::GpuPaint;
use crate::renderer::gpu_paint::gpu_frame_ctx::GpuFrameCtx;
use crate::renderer::gpu_paint::gpu_paint_ref::GpuPaintRef;
use glam::Vec2;
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn polyline_payload_predicate_uses_the_canonical_scalar_noop_policy() {
    use crate::primitives::approx::EPS;

    #[derive(Debug)]
    struct Case {
        points_len: u32,
        width: f32,
        expected_noop: bool,
    }

    let cases = [
        Case {
            points_len: 0,
            width: 1.0,
            expected_noop: true,
        },
        Case {
            points_len: 1,
            width: 1.0,
            expected_noop: true,
        },
        Case {
            points_len: 2,
            width: -1.0,
            expected_noop: true,
        },
        Case {
            points_len: 2,
            width: 0.0,
            expected_noop: true,
        },
        Case {
            points_len: 2,
            width: EPS * 0.5,
            expected_noop: true,
        },
        Case {
            points_len: 2,
            width: f32::NAN,
            expected_noop: true,
        },
        Case {
            points_len: 2,
            width: EPS * 2.0,
            expected_noop: false,
        },
    ];

    for case in cases {
        let payload = DrawPolylinePayload {
            points_len: case.points_len,
            width: case.width,
            ..Default::default()
        };
        assert_eq!(payload.is_noop(), case.expected_noop, "{case:?}");
    }
}

/// The `GpuView` gate. A collapsed view emits *nothing*: no image
/// draw and no callback, so the composer schedules no off-screen
/// target for a widget that can't paint. A live one records the
/// payload and its callback as a single call — the composite and the
/// target it needs can't come apart.
///
/// The null-handle rows are what say the two callers of that arm are
/// told apart by the `paint` argument alone. `TextureId(0)` means
/// "no texture to sample" for a registered image and is dropped; for
/// a `GpuView` it means nothing, because the target is
/// framework-painted this frame rather than registered. Production
/// never mints `TextureId(0)` for a view, but the branch decides on
/// `paint.is_some()` and nothing else would notice if it stopped.
#[test]
fn gpu_view_gate_drops_zero_extent_and_pairs_payload_with_paint() {
    #[derive(Debug)]
    struct NoopGpuPaint;

    impl GpuPaint for NoopGpuPaint {
        fn paint(&mut self, _ctx: &mut GpuFrameCtx<'_>) {}
    }

    let paint = GpuPaintRef(Rc::new(RefCell::new(NoopGpuPaint)));
    let live = Rect::new(1.0, 2.0, 10.0, 10.0);
    let cases = [
        (
            "zero_width",
            Rect::new(0.0, 0.0, 0.0, 10.0),
            TextureId(7),
            true,
            false,
        ),
        (
            "zero_height",
            Rect::new(0.0, 0.0, 10.0, 0.0),
            TextureId(7),
            true,
            false,
        ),
        ("live", live, TextureId(7), true, true),
        // The two halves of the null-handle arm, which is the whole
        // reason the gate needs to know about the callback at all.
        ("null_handle_image", live, TextureId(0), false, false),
        ("null_handle_view", live, TextureId(0), true, true),
    ];

    for (label, rect, handle, has_paint, expect_call) in cases {
        let mut sink = PaintCapture::default();
        sink.draw_image(
            DrawImagePayload::image(
                rect,
                Vec2::ZERO,
                Vec2::ONE,
                ColorF16::from(Color::WHITE),
                handle,
                0,
            ),
            has_paint.then_some(&paint),
        );
        if !expect_call {
            assert!(sink.calls.is_empty(), "case {label}: {:?}", sink.calls);
            continue;
        }
        let [
            PaintCall::Image {
                payload,
                paint: got,
            },
        ] = sink.calls.as_slice()
        else {
            panic!(
                "case {label}: expected one Image call, got {:?}",
                sink.calls
            );
        };
        assert_eq!(got.as_ref(), Some(&paint), "case {label}");
        assert_eq!(payload.rect, rect, "case {label}");
        assert_eq!(payload.handle, handle, "case {label}");
        assert_eq!(payload.uv_min, Vec2::ZERO, "case {label}");
        assert_eq!(payload.uv_size, Vec2::ONE, "case {label}");
        assert_eq!(payload.flags, 0, "case {label}");
        assert_eq!(payload.tint, ColorF16::from(Color::WHITE), "case {label}");
    }
}
