//! The encoder's output surface.
//!
//! [`PaintSink`] is the one interface the [`Encoder`] paints through.
//! In production the only sink is `ComposeSession`, which composes each
//! call straight into a `RenderBuffer` — there is no intermediate
//! command stream. Tests and benches add a recording sink
//! (`record_sink`) that captures the same calls as owned values.
//!
//! The trait splits in two halves. **Required** methods take a fully
//! lowered payload — one per paint operation, matching what the composer
//! consumes. **Provided** methods are the encoder-facing surface: they
//! own the no-op gates and the brush/stroke lowering, then call down.
//! Keeping that half here rather than in either sink is what keeps the
//! no-op policy single-copy — it can't drift between the two sinks
//! because there is only one copy of it. It is not *unbypassable*: the
//! required half is crate-visible, so `sink.rect(payload)` compiles
//! anywhere and skips the gate. `RecordedPaint::replay` is the one
//! place that does so, and only because its input already passed.
//!
//! ## Noop policy
//!
//! Every `draw_*` early-returns when its inputs would emit no visible
//! pixels (transparent fill color, no-op stroke, no-op shadow tint).
//! **The provided half is the single canonical gate** — callers
//! don't need to pre-check, and the encoder doesn't gate per branch.
//! Upstream filters (`Shape::is_noop` at `Ui::add_shape`,
//! whole-`Background::is_noop` at `Tree::open_node`) are performance
//! optimizations that skip expensive lowering (text shaping, payload
//! staging) or sparse-column writes, not correctness gates.
//!
//! Exception: [`PaintSink::draw_polyline`] doesn't gate on colour. Its
//! colors live in spans (`PerSegment` can mix one solid stop with N
//! transparent), and an O(n) read on every emit would dominate the
//! per-call cost. Colour noops are caught by `Shape::Polyline::is_noop`
//! at the authoring boundary instead; the payload's own `is_noop` still
//! gates degenerate geometry (point count / width).
//!
//! [`Encoder`]: crate::renderer::frontend::encoder::Encoder

use crate::primitives::brush::gradient::FillAxis;
use crate::primitives::color::ColorF16;
use crate::primitives::corners::Corners;
use crate::primitives::fill_wire::FillKind;
use crate::primitives::rect::Rect;
use crate::primitives::texture_id::TextureId;
use crate::primitives::transform::TranslateScale;
use crate::renderer::frontend::payload::{
    BrushSource, DrawCurvePayload, DrawImagePayload, DrawMeshPayload, DrawPolylinePayload,
    DrawRectPayload, DrawShadowPayload, DrawTextPayload, DrawTrianglePayload, GpuFillFields,
    PushClipPayload,
};
use crate::renderer::gpu_view::GpuPaintRef;
use crate::scene::shapes::paint::ShapeStroke;
use crate::text::key::ShapedTextRef;

/// Sink for one frame's lowered paint operations, in authoring order.
/// See the module docs for the required/provided split.
pub(crate) trait PaintSink {
    /// Push a clip region. `payload.corners` is zero for a rect clip.
    fn clip(&mut self, payload: PushClipPayload);

    fn pop_clip(&mut self);

    fn push_transform(&mut self, transform: TranslateScale);

    fn pop_transform(&mut self);

    fn rect(&mut self, payload: DrawRectPayload);

    fn shadow(&mut self, payload: DrawShadowPayload);

    fn text(&mut self, payload: DrawTextPayload);

    fn mesh(&mut self, payload: DrawMeshPayload);

    fn polyline(&mut self, payload: DrawPolylinePayload);

    /// `paint` is `Some` exactly when this image composites a `GpuView`,
    /// carrying the app callback the off-screen target is painted with.
    fn image(&mut self, payload: DrawImagePayload, paint: Option<&GpuPaintRef>);

    fn curve(&mut self, payload: DrawCurvePayload);

    fn triangle(&mut self, payload: DrawTrianglePayload);

    #[inline]
    fn push_clip(&mut self, rect: Rect) {
        self.clip(PushClipPayload {
            rect,
            corners: Corners::ZERO,
        });
    }

    #[inline]
    fn push_clip_rounded(&mut self, rect: Rect, corners: Corners) {
        self.clip(PushClipPayload { rect, corners });
    }

    #[inline]
    fn draw_rect(&mut self, rect: Rect, corners: Corners, fill: BrushSource, stroke: ShapeStroke) {
        self.draw_rect_impl(rect, corners, fill, stroke, false);
    }

    /// Windowed-rect sibling of [`Self::draw_rect`]: same payload, but
    /// the `FillKind` carries the window bit so the shader inverts the
    /// fill coverage (fill outside the rounded boundary, transparent
    /// window inside the stroke). The bit also keeps the composer's
    /// opaque-cover checks (`fill_kind == FillKind::SOLID`) from
    /// treating the quad as an occluder — its interior is a hole.
    #[inline]
    fn draw_rect_window(
        &mut self,
        rect: Rect,
        corners: Corners,
        fill: BrushSource,
        stroke: ShapeStroke,
    ) {
        self.draw_rect_impl(rect, corners, fill, stroke, true);
    }

    #[inline]
    fn draw_rect_impl(
        &mut self,
        rect: Rect,
        corners: Corners,
        fill: BrushSource,
        stroke: ShapeStroke,
        window: bool,
    ) {
        if rect.is_paint_empty() || (fill.is_noop() && stroke.is_noop()) {
            return;
        }

        // Stroke stays solid-only — gradient strokes are a non-goal.
        let GpuFillFields {
            color: fill_color,
            kind: fill_kind,
            lut_row: fill_lut_row,
            axis: fill_axis,
        } = fill.to_gpu_fields();
        let fill_kind = if window {
            fill_kind.with_window()
        } else {
            fill_kind
        };

        let (stroke_color, stroke_width) = if stroke.is_noop() {
            (ColorF16::TRANSPARENT, 0.0)
        } else {
            (stroke.color, stroke.width())
        };
        self.rect(DrawRectPayload {
            rect,
            corners,
            fill: fill_color,
            stroke_color,
            stroke_width,
            fill_kind,
            fill_lut_row,
            fill_axis,
        });
    }

    /// Paint a shadow. For a drop shadow, `rect` is the offset source
    /// inflated by `3σ + max(spread, 0)`; for an inset shadow it is the
    /// source rect. `corners` is the source shape's corner radii,
    /// `color` the shadow tint, `fill_kind`
    /// `FillKind::SHADOW_DROP|SHADOW_INSET`. Drop shadows carry
    /// `(0, 0, σ, spread)` in `fill_axis`; inset shadows carry
    /// `(offset.x, offset.y, σ, spread)`. The composer scales the
    /// logical-px lanes to physical px on emit.
    #[inline]
    fn draw_shadow(
        &mut self,
        rect: Rect,
        corners: Corners,
        color: ColorF16,
        fill_kind: FillKind,
        fill_axis: FillAxis,
    ) {
        let payload = DrawShadowPayload {
            rect,
            corners,
            color,
            fill_kind,
            fill_axis,
        };
        if payload.is_noop() {
            return;
        }
        self.shadow(payload);
    }

    #[inline]
    fn draw_text(&mut self, rect: Rect, color: ColorF16, text: ShapedTextRef) {
        let payload = DrawTextPayload { rect, color, text };
        if payload.is_noop() {
            return;
        }
        self.text(payload);
    }

    /// Paint a mesh against already-staged vertices + indices in
    /// `RecordStore.meshes`. The recorder pushes verts (translated into
    /// the owner's logical-px world coords) and indices directly, so the
    /// encoder applies the owner-rect offset inline without an
    /// intermediate scratch buffer.
    fn draw_mesh(&mut self, payload: DrawMeshPayload) {
        if payload.is_noop() {
            return;
        }
        self.mesh(payload);
    }

    fn draw_image(&mut self, payload: DrawImagePayload) {
        if payload.is_noop() {
            return;
        }
        self.image(payload, None);
    }

    /// Composite a `GpuView` over its full arranged `rect` and schedule
    /// its off-screen paint. Same draw as any image — the callback rides
    /// alongside the payload so the sink can list the off-screen target.
    fn draw_gpu_view(&mut self, rect: Rect, handle: TextureId, paint: &GpuPaintRef) {
        let payload = DrawImagePayload::gpu_view(rect, handle);
        if payload.is_noop() {
            return;
        }
        self.image(payload, Some(paint));
    }

    fn draw_curve(&mut self, payload: DrawCurvePayload) {
        if payload.is_noop() {
            return;
        }
        self.curve(payload);
    }

    /// Paint a rounded triangle from three owner-local `points` offset
    /// by `origin`. Noop strokes normalize to `(TRANSPARENT, 0.0)`,
    /// exactly like [`Self::draw_rect`].
    fn draw_triangle(
        &mut self,
        origin: glam::Vec2,
        points: [glam::Vec2; 3],
        fill: ColorF16,
        radius: f32,
        stroke: ShapeStroke,
    ) {
        let (stroke_color, stroke_width) = if stroke.is_noop() {
            (ColorF16::TRANSPARENT, 0.0)
        } else {
            (stroke.color, stroke.width())
        };
        let [a, b, c] = points;
        let payload = DrawTrianglePayload {
            origin,
            a,
            b,
            c,
            fill,
            stroke_color,
            radius,
            stroke_width,
        };
        if payload.is_noop() {
            return;
        }
        self.triangle(payload);
    }

    /// Paint a polyline against already-staged points and colors. The
    /// recorder pushes onto `polyline_points` / `polyline_colors`
    /// directly (so the encoder can apply the owner-rect offset inline
    /// without an intermediate scratch buffer) and passes the resulting
    /// spans in the payload. The `color_mode`-dictated `colors_len` is a
    /// caller invariant enforced upstream by
    /// `PolylineColors::assert_matches` in `Shapes::add`.
    fn draw_polyline(&mut self, payload: DrawPolylinePayload) {
        if payload.is_noop() {
            return;
        }
        self.polyline(payload);
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::color::{Color, ColorF16};
    use crate::primitives::corners::Corners;
    use crate::primitives::rect::Rect;

    use crate::primitives::stroke::Stroke;
    use crate::primitives::texture_id::TextureId;
    use crate::renderer::frontend::paint_sink::PaintSink;
    use crate::renderer::frontend::payload::{BrushSource, DrawPolylinePayload};
    use crate::renderer::frontend::record_sink::{PaintCall, RecordedPaint};
    use crate::renderer::gpu_view::{GpuFrameCtx, GpuPaint, GpuPaintRef};
    use crate::scene::shapes::paint::ShapeStroke;
    use glam::Vec2;
    use std::cell::RefCell;
    use std::rc::Rc;

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
                ..Default::default()
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
            let mut rb = RecordedPaint::default();
            rb.draw_rect(
                Rect::new(0.0, 0.0, 10.0, 10.0),
                Corners::ZERO,
                BrushSource::Solid(fill.into()),
                stroke,
            );
            let Some(PaintCall::Rect(rp)) = rb.calls.first() else {
                panic!("case {label}: expected DrawRect");
            };

            let mut tb = RecordedPaint::default();
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
            let Some(PaintCall::Triangle(tp)) = tb.calls.first() else {
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

    /// The `GpuView` gate. A collapsed view emits *nothing*: no image
    /// draw and no callback, so the composer schedules no off-screen
    /// target for a widget that can't paint. A live one records the
    /// payload and its callback as a single call — the composite and the
    /// target it needs can't come apart. The null-handle arm of
    /// `is_noop` is deliberately unreachable here: a `GpuView`'s texture
    /// is framework-painted and `TextureId(0)` is never minted.
    #[test]
    fn gpu_view_gate_drops_zero_extent_and_pairs_payload_with_paint() {
        #[derive(Debug)]
        struct NoopGpuPaint;

        impl GpuPaint for NoopGpuPaint {
            fn paint(&mut self, _ctx: &mut GpuFrameCtx<'_>) {}
        }

        let paint = GpuPaintRef(Rc::new(RefCell::new(NoopGpuPaint)));
        let handle = TextureId(7);
        let cases = [
            ("zero_width", Rect::new(0.0, 0.0, 0.0, 10.0), false),
            ("zero_height", Rect::new(0.0, 0.0, 10.0, 0.0), false),
            ("live", Rect::new(1.0, 2.0, 10.0, 10.0), true),
        ];

        for (label, rect, expect_call) in cases {
            let mut sink = RecordedPaint::default();
            sink.draw_gpu_view(rect, handle, &paint);
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
}
