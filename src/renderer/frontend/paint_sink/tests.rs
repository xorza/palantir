//! The paint sink's no-op gate: what it drops and passes.

use crate::internals::paint_capture::{PaintCall, PaintCapture};
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::draw_image_payload::{
    DrawImagePayload, ImageDraw, ViewPaint,
};
use crate::renderer::frontend::payload::draw_polyline_payload::DrawPolylinePayload;
use crate::renderer::gpu_paint::gpu_paint_ref::GpuPaintRef;
use crate::renderer::render_buffer::image_flags::ImageFlags;
use glam::Vec2;

#[test]
fn polyline_payload_predicate_uses_the_canonical_scalar_noop_policy() {
    use crate::primitives::math::domain::EPS;

    #[derive(Debug)]
    struct Case {
        points_len: u32,
        width: f32,
        alpha: f32,
        expected_noop: bool,
    }

    let cases = [
        Case {
            points_len: 0,
            width: 1.0,
            alpha: 1.0,
            expected_noop: true,
        },
        Case {
            points_len: 1,
            width: 1.0,
            alpha: 1.0,
            expected_noop: true,
        },
        Case {
            points_len: 2,
            width: -1.0,
            alpha: 1.0,
            expected_noop: true,
        },
        Case {
            points_len: 2,
            width: 0.0,
            alpha: 1.0,
            expected_noop: true,
        },
        Case {
            points_len: 2,
            width: EPS * 0.5,
            alpha: 1.0,
            expected_noop: true,
        },
        Case {
            points_len: 2,
            width: f32::NAN,
            alpha: 1.0,
            expected_noop: true,
        },
        Case {
            points_len: 2,
            width: EPS * 2.0,
            alpha: 1.0,
            expected_noop: false,
        },
        // Faded to nothing, and below the paint threshold.
        Case {
            points_len: 2,
            width: 1.0,
            alpha: 0.0,
            expected_noop: true,
        },
        Case {
            points_len: 2,
            width: 1.0,
            alpha: EPS * 0.5,
            expected_noop: true,
        },
    ];

    for case in cases {
        let payload = DrawPolylinePayload {
            points_len: case.points_len,
            width: case.width,
            alpha: case.alpha,
            ..Default::default()
        };
        assert_eq!(payload.is_noop(), case.expected_noop, "{case:?}");
    }
}

/// A collapsed `GpuView` emits nothing: no image draw, no callback, so no off-screen target is scheduled. A live one records payload and callback as one call.
///
/// The null-handle rows pin that the two callers of that arm differ by `paint` alone: `TextureId(0)` means "no texture" for an image (dropped) but nothing for a `GpuView`.
#[test]
fn gpu_view_gate_drops_zero_extent_and_pairs_payload_with_paint() {
    let paint = GpuPaintRef::noop();
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
        // The two halves of the null-handle arm.
        ("null_handle_image", live, TextureId(0), false, false),
        ("null_handle_view", live, TextureId(0), true, true),
    ];

    for (label, rect, handle, has_paint, expect_call) in cases {
        let mut sink = PaintCapture::default();
        sink.draw_image(
            ImageDraw {
                payload: DrawImagePayload {
                    rect,
                    uv_min: Vec2::ZERO,
                    uv_size: Vec2::ONE,
                    tint: RgbaF16::from(RgbaF32::WHITE),
                    handle,
                    flags: ImageFlags::NONE,
                },
                view: has_paint.then_some(ViewPaint {
                    paint: &paint,
                    epoch: 7,
                }),
            },
            1.0,
        );
        if !expect_call {
            assert!(sink.calls.is_empty(), "case {label}: {:?}", sink.calls);
            continue;
        }
        let [
            PaintCall::Image {
                payload,
                paint: got,
                epoch,
            },
        ] = sink.calls.as_slice()
        else {
            panic!(
                "case {label}: expected one Image call, got {:?}",
                sink.calls
            );
        };
        assert_eq!(got.as_ref(), Some(&paint), "case {label}");
        assert_eq!(*epoch, 7, "case {label}: the capture keeps the epoch");
        assert_eq!(payload.rect, rect, "case {label}");
        assert_eq!(payload.handle, handle, "case {label}");
        assert_eq!(payload.uv_min, Vec2::ZERO, "case {label}");
        assert_eq!(payload.uv_size, Vec2::ONE, "case {label}");
        assert_eq!(payload.flags, ImageFlags::NONE, "case {label}");
        assert_eq!(payload.tint, RgbaF16::from(RgbaF32::WHITE), "case {label}");
    }
}

/// The gate sees the faded payload. A draw faded to nothing is dropped by the existing no-op gate; a half fade halves the tint alpha and leaves colour lanes untouched.
#[test]
fn the_gate_sees_the_faded_payload() {
    let draw = || ImageDraw {
        payload: DrawImagePayload {
            rect: Rect::new(0.0, 0.0, 4.0, 4.0),
            uv_min: Vec2::ZERO,
            uv_size: Vec2::ONE,
            tint: RgbaF16::from(RgbaF32::WHITE),
            handle: TextureId(7),
            flags: ImageFlags::NONE,
        },
        view: None,
    };

    let mut faded_out = PaintCapture::default();
    faded_out.draw_image(draw(), 0.0);
    assert!(
        faded_out.calls.is_empty(),
        "a draw animated to nothing must not reach the sink: {:?}",
        faded_out.calls,
    );

    let mut half = PaintCapture::default();
    half.draw_image(draw(), 0.5);
    let [PaintCall::Image { payload, .. }] = half.calls.as_slice() else {
        panic!("expected one Image call, got {:?}", half.calls);
    };
    // Half an opaque white tint's alpha, colour untouched; exact in f16.
    let tint = payload.tint.unpack();
    assert_eq!([tint.r, tint.g, tint.b, tint.a], [1.0, 1.0, 1.0, 0.5]);

    // A polyline gates alike: its fade rides the payload's alpha lane, its colours being in the record store.
    let line = DrawPolylinePayload {
        points_len: 2,
        width: 1.0,
        alpha: 1.0,
        ..Default::default()
    };
    let mut faded_out = PaintCapture::default();
    faded_out.draw_polyline(line, 0.0);
    assert!(
        faded_out.calls.is_empty(),
        "a polyline animated to nothing must not reach the sink: {:?}",
        faded_out.calls,
    );
    let mut half = PaintCapture::default();
    half.draw_polyline(line, 0.5);
    let [PaintCall::Polyline(payload)] = half.calls.as_slice() else {
        panic!("expected one Polyline call, got {:?}", half.calls);
    };
    assert_eq!(payload.alpha, 0.5);
}
