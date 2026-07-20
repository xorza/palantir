use crate::primitives::color::{Color, ColorF16};
use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::primitives::stroke::Stroke;
use crate::renderer::frontend::cmd_buffer::payload::{BrushSource, CmdKind, DrawPolylinePayload};
use crate::renderer::frontend::cmd_buffer::{
    COMMAND_KIND_BITS, COMMAND_KIND_MASK, Command, MAX_DATA_WORD_OFFSET, RenderCmdBuffer,
    pack_command_descriptor,
};
use crate::renderer::gpu_view::{GpuFrameCtx, GpuPaint, GpuPaintRef};
use crate::renderer::texture_id::TextureId;
use crate::scene::shapes::paint::ShapeStroke;
use glam::Vec2;
use std::cell::RefCell;
use std::rc::Rc;
use strum::{EnumCount as _, IntoEnumIterator};

#[derive(Debug)]
struct NoopPaint;

impl GpuPaint for NoopPaint {
    fn paint(&mut self, _ctx: &mut GpuFrameCtx<'_>) {}
}

fn paint() -> GpuPaintRef {
    GpuPaintRef(Rc::new(RefCell::new(NoopPaint)))
}

#[test]
fn descriptor_round_trips_every_kind_and_offset_boundary() {
    let mut buffer = RenderCmdBuffer::default();
    for (expected_tag, kind) in CmdKind::iter().enumerate() {
        let descriptor = pack_command_descriptor(kind, MAX_DATA_WORD_OFFSET);
        assert_eq!(
            descriptor & COMMAND_KIND_MASK,
            expected_tag as u32,
            "{kind:?} tag",
        );
        assert_eq!(
            descriptor >> COMMAND_KIND_BITS,
            MAX_DATA_WORD_OFFSET as u32,
            "{kind:?} maximum offset",
        );
        assert_eq!(
            CmdKind::from_repr((descriptor & COMMAND_KIND_MASK) as u8),
            Some(kind),
            "{kind:?} round trip",
        );
        buffer.record_start(kind);
    }
    assert_eq!(
        buffer.descriptors.len() * size_of::<u32>(),
        CmdKind::COUNT * 4,
        "command metadata must use four bytes per command",
    );
    assert_eq!(
        MAX_DATA_WORD_OFFSET * size_of::<u32>(),
        (1 << 30) - 4,
        "the maximum word offset must address the final aligned word below 1 GiB",
    );
    assert!(
        std::panic::catch_unwind(|| {
            pack_command_descriptor(CmdKind::PushClip, MAX_DATA_WORD_OFFSET + 1);
        })
        .is_err(),
        "the first unrepresentable word offset must be rejected",
    );
}

#[test]
fn gpu_view_records_payload_and_paint_atomically() {
    let mut buffer = RenderCmdBuffer::default();
    buffer.draw_gpu_view(Rect::new(2.0, 3.0, 0.0, 8.0), TextureId(1), paint());
    assert_eq!(buffer.iter().len(), 0);
    assert!(buffer.gpu_view_paints.is_empty());

    buffer.draw_gpu_view(Rect::new(2.0, 3.0, 7.0, 8.0), TextureId(1), paint());
    assert_eq!(buffer.gpu_view_paints.len(), 1);
    let command = buffer.iter().next().expect("GpuView command missing");
    match command {
        Command::DrawImage {
            payload,
            paint: Some(_),
        } => assert_eq!(payload.rect, Rect::new(2.0, 3.0, 7.0, 8.0)),
        other => panic!("expected linked GpuView image command, got {other:?}"),
    }
}

#[test]
fn polyline_payload_gate_uses_the_canonical_scalar_noop_policy() {
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
            ..bytemuck::Zeroable::zeroed()
        };
        assert_eq!(payload.is_noop(), case.expected_noop, "{case:?}");
    }
}

/// Both draw paths run the same stroke normalization inside the cmd
/// buffer (the single canonical correctness gate): a noop stroke —
/// transparent colour or zero width — lands in the payload as
/// `(TRANSPARENT, 0.0)`; anything else passes through verbatim. NaN
/// width is NOT a `ShapeStroke` noop (`noop_f16_bits` deliberately
/// classifies NaN as non-zero — loud bug, not silent skip), so it
/// passes through on both paths identically and the shader's
/// `stroke_width > 0.0` gate drops it. Table sweeps all four cases,
/// asserting the triangle payload's stroke fields are bit-identical to
/// the rect payload's for the same input stroke.
#[test]
fn triangle_stroke_normalization_matches_draw_rect() {
    let fill = Color::rgb(1.0, 0.0, 0.0);
    let green = Color::rgb(0.0, 1.0, 0.0);
    let cases: [(&str, ShapeStroke, bool); 4] = [
        (
            "transparent_color",
            Stroke::solid(Color::TRANSPARENT, 3.0).into(),
            true,
        ),
        ("zero_width", Stroke::solid(green, 0.0).into(), true),
        ("nan_width", Stroke::solid(green, f32::NAN).into(), false),
        ("live", Stroke::solid(green, 3.0).into(), false),
    ];
    for (label, stroke, expect_normalized) in cases {
        let mut rb = RenderCmdBuffer::default();
        rb.draw_rect(
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Corners::ZERO,
            BrushSource::Solid(fill.into()),
            stroke,
        );
        let Some(Command::DrawRect(rp)) = rb.iter().next() else {
            panic!("case {label}: expected DrawRect");
        };

        let mut tb = RenderCmdBuffer::default();
        tb.draw_triangle(
            Vec2::ZERO,
            [
                Vec2::new(0.0, 0.0),
                Vec2::new(10.0, 0.0),
                Vec2::new(5.0, 8.0),
            ],
            fill.into(),
            0.0,
            stroke,
        );
        let Some(Command::DrawTriangle(tp)) = tb.iter().next() else {
            panic!("case {label}: expected DrawTriangle");
        };

        assert_eq!(tp.stroke_color, rp.stroke_color, "case {label}");
        assert_eq!(
            tp.stroke_width.to_bits(),
            rp.stroke_width.to_bits(),
            "case {label}",
        );
        if expect_normalized {
            assert_eq!(tp.stroke_color, ColorF16::TRANSPARENT, "case {label}");
            assert_eq!(tp.stroke_width, 0.0, "case {label}");
        } else {
            assert_eq!(tp.stroke_color, ColorF16::from(green), "case {label}");
        }
    }
}
