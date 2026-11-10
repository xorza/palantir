//! The paint sink's no-op gate: what it drops and passes.

use crate::common::span::Span;
use crate::internals::paint_capture::{PaintCall, PaintCapture};
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::fit::FitMap;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::line_style::LineJoin;
use crate::renderer::frontend::paint_sink::PaintPayload;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::draw_closed_payload::DrawClosedPayload;
use crate::renderer::frontend::payload::draw_closed_payload::closed_image::{
    ClosedImage, GpuViewDraw,
};
use crate::renderer::frontend::payload::draw_stroke_payload::{
    DrawStrokePayload, PolylinePath, StrokePath,
};
use crate::renderer::frontend::payload::gpu_fill::GpuFill;
use crate::renderer::render_buffer::image_flags::ImageFlags;
use crate::shape::geometry::box_geometry::PlacedBox;
use crate::shape::geometry::closed_geometry::PlacedClosed;
use crate::shape::geometry::stroke_geometry::PolylineColorSpan;
use glam::Vec2;

/// A white polyline stroke of `points_len` points from the store's start, `width` wide, at
/// `alpha`.
fn polyline(points_len: u32, width: f32, alpha: f32) -> DrawStrokePayload {
    DrawStrokePayload {
        path: StrokePath::Polyline(PolylinePath {
            bbox: Rect::ZERO,
            fit: FitMap::IDENTITY,
            points: Span::new(0, points_len),
            colors: PolylineColorSpan::Single,
            join: LineJoin::Miter,
        }),
        fill: GpuFill {
            color: RgbaF32::new(1.0, 1.0, 1.0, alpha).to_f16(),
            ..GpuFill::default()
        },
        width,
        ..DrawStrokePayload::default()
    }
}

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
        let payload = polyline(case.points_len, case.width, case.alpha);
        assert_eq!(payload.is_noop(), case.expected_noop, "{case:?}");
    }
}

/// A collapsed `GpuView` emits nothing: no image draw, no view, so no off-screen target is scheduled. A live one records its payload with its view.
///
/// The null-handle rows pin that the two callers of that arm differ by `view` alone: `TextureId(0)` means "no texture" for an image (dropped) but nothing for a `GpuView`.
#[test]
fn gpu_view_gate_drops_zero_extent_and_keeps_the_view() {
    let view = GpuViewDraw {
        owner: WidgetId::from_hash("view"),
        epoch: 7,
    };
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
        sink.draw(
            DrawClosedPayload {
                geometry: PlacedClosed::Box(PlacedBox {
                    rect,
                    corners: Corners::ZERO,
                }),
                spin: None,
                paint: ClosedImage {
                    rect,
                    uv_min: Vec2::ZERO,
                    uv_size: Vec2::ONE,
                    tint: RgbaF32::WHITE.to_f16(),
                    handle,
                    flags: ImageFlags::NONE,
                    view: has_paint.then_some(view),
                },
            },
            1.0,
        );
        if !expect_call {
            assert!(sink.calls.is_empty(), "case {label}: {:?}", sink.calls);
            continue;
        }
        let [PaintCall::Image(payload)] = sink.calls.as_slice() else {
            panic!(
                "case {label}: expected one Image call, got {:?}",
                sink.calls
            );
        };
        assert_eq!(
            payload.paint.view,
            Some(view),
            "case {label}: the capture keeps the view"
        );
        assert_eq!(payload.paint.rect, rect, "case {label}");
        assert_eq!(payload.paint.handle, handle, "case {label}");
        assert_eq!(payload.paint.uv_min, Vec2::ZERO, "case {label}");
        assert_eq!(payload.paint.uv_size, Vec2::ONE, "case {label}");
        assert_eq!(payload.paint.flags, ImageFlags::NONE, "case {label}");
        assert_eq!(payload.paint.tint, RgbaF32::WHITE.to_f16(), "case {label}");
    }
}

/// The gate sees the faded payload. A draw faded to nothing is dropped by the existing no-op gate; a half fade halves the tint alpha and leaves color lanes untouched.
#[test]
fn the_gate_sees_the_faded_payload() {
    let draw = || DrawClosedPayload {
        geometry: PlacedClosed::Box(PlacedBox {
            rect: Rect::new(0.0, 0.0, 4.0, 4.0),
            corners: Corners::ZERO,
        }),
        spin: None,
        paint: ClosedImage {
            rect: Rect::new(0.0, 0.0, 4.0, 4.0),
            uv_min: Vec2::ZERO,
            uv_size: Vec2::ONE,
            tint: RgbaF32::WHITE.to_f16(),
            handle: TextureId(7),
            flags: ImageFlags::NONE,
            view: None,
        },
    };

    let mut faded_out = PaintCapture::default();
    faded_out.draw(draw(), 0.0);
    assert!(
        faded_out.calls.is_empty(),
        "a draw animated to nothing must not reach the sink: {:?}",
        faded_out.calls,
    );

    let mut half = PaintCapture::default();
    half.draw(draw(), 0.5);
    let [PaintCall::Image(payload)] = half.calls.as_slice() else {
        panic!("expected one Image call, got {:?}", half.calls);
    };
    // Half an opaque white tint's alpha, color untouched; exact in f16.
    let tint = RgbaF32::from(payload.paint.tint);
    assert_eq!([tint.r, tint.g, tint.b, tint.a], [1.0, 1.0, 1.0, 0.5]);

    // A polyline gates alike: its fade rides its fill's alpha, which its colors in the record store
    // multiply.
    let line = polyline(2, 1.0, 1.0);
    let mut faded_out = PaintCapture::default();
    faded_out.draw(line, 0.0);
    assert!(
        faded_out.calls.is_empty(),
        "a polyline animated to nothing must not reach the sink: {:?}",
        faded_out.calls,
    );
    let mut half = PaintCapture::default();
    half.draw(line, 0.5);
    let [PaintCall::Stroke(payload)] = half.calls.as_slice() else {
        panic!("expected one Stroke call, got {:?}", half.calls);
    };
    assert_eq!(RgbaF32::from(payload.fill.color).a, 0.5);
}

/// The gate turns what it fades: a draw handed a turn reaches the sink with it, a polyline's
/// alike, and a fade alone, an `f32`, leaves a draw as it was turned, still or not.
#[test]
fn the_gate_turns_the_payload() {
    use crate::renderer::frontend::paint_sink::DrawAnimation;
    use crate::renderer::frontend::payload::spin::Spin;

    let turn = Spin {
        pivot: Vec2::new(2.0, 2.0),
        angle: 0.5,
    };
    let image = DrawClosedPayload::new(
        PlacedClosed::Box(PlacedBox {
            rect: Rect::new(0.0, 0.0, 4.0, 4.0),
            corners: Corners::ZERO,
        }),
        ClosedImage {
            rect: Rect::new(0.0, 0.0, 4.0, 4.0),
            uv_min: Vec2::ZERO,
            uv_size: Vec2::ONE,
            tint: RgbaF32::WHITE.to_f16(),
            handle: TextureId(7),
            flags: ImageFlags::NONE,
            view: None,
        },
    );
    let line = polyline(2, 1.0, 1.0);

    let mut sink = PaintCapture::default();
    let animation = DrawAnimation {
        alpha: 1.0,
        spin: Some(turn),
    };
    sink.draw(image, animation);
    sink.draw(line, animation);
    sink.draw(image, 1.0);
    sink.draw(image.turned(Some(turn)), 1.0);
    let spins: Vec<_> = sink
        .calls
        .iter()
        .map(|call| match call {
            PaintCall::Image(p) => p.spin,
            PaintCall::Stroke(p) => p.spin,
            other => panic!("unexpected call {other:?}"),
        })
        .collect();
    assert_eq!(spins, [Some(turn), Some(turn), None, Some(turn)]);
}
