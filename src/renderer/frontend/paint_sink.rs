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
//! gate single-copy — it can't drift between the two sinks because
//! there is only one copy of it.
//!
//! ## Noop policy
//!
//! **The canonical statement of the tier policy — the whole pipeline's,
//! not just this module's.** Other tiers point here rather than restate
//! it; what they document locally is which *values* they consider
//! invisible, never the policy.
//!
//! `is_noop` appears at three tiers, and they are not redundant with
//! each other — each answers a different question at a point where the
//! others cannot:
//!
//! 1. **Primitives** (`Color`, `Stroke`, `Shadow`, `Brush`,
//!    `TranslateScale`, …) answer "is this *value* invisible". They are
//!    the vocabulary the other two tiers are written in.
//! 2. **Authoring shapes** (`Shape::is_noop`, at `Shapes::add`) compose
//!    those to skip *lowering* — text shaping, payload staging, mesh
//!    hashing. A payload-level gate cannot do this job: by the time a
//!    payload exists, the work being skipped has already happened.
//!    `Background::is_noop` at `Tree::open_node` is the same tier for
//!    chrome, skipping a sparse-column write.
//! 3. **Lowered payloads** (`Draw*Payload::is_noop`, called from this
//!    trait's provided half) are the **single correctness gate**.
//!    Callers don't pre-check and the encoder doesn't gate per branch;
//!    everything funnels here.
//!
//! So tier 2 is an optimization and tier 3 is correctness — a shape
//! that slips past tier 2 still paints nothing, but pays for lowering.
//! The gate is not *unbypassable*: the required half is crate-visible,
//! so `sink.quad(payload)` compiles anywhere and skips it.
//! `RecordedPaint::replay` is the one place that does, and only because
//! its input already passed.
//!
//! Exception: [`PaintSink::draw_polyline`] gates on nothing, and
//! *asserts* instead. Its colours live in spans (`PerSegment` can mix
//! one solid stop with N transparent), so an O(n) read on every emit
//! would dominate the per-call cost — those are caught by
//! `Shape::Polyline::is_noop` at tier 2. Its geometry conditions are
//! caught there too, and unlike every other payload's they are
//! authoring-derived, so nothing between the two tiers can invalidate
//! them. That makes a degenerate polyline here a broken contract rather
//! than a value to filter, which is what an assert says and a silent
//! `return` does not.
//!
//! [`Encoder`]: crate::renderer::frontend::encoder::Encoder

use crate::primitives::brush::gradient::FillAxis;
use crate::primitives::color::ColorF16;
use crate::primitives::corners::Corners;
use crate::primitives::fill_wire::{FillKind, LutRow};
use crate::primitives::rect::Rect;
use crate::primitives::transform::TranslateScale;
use crate::renderer::frontend::payload::{
    BrushSource, DrawCurvePayload, DrawImagePayload, DrawMeshPayload, DrawPolylinePayload,
    DrawQuadPayload, DrawTextPayload, GpuFillFields, PushClipPayload, QuadGeom,
};
use crate::renderer::gpu_view::GpuPaintRef;
use crate::scene::shapes::paint::ShapeStroke;
use crate::text::shaped_ref::ShapedTextRef;

/// Sink for one frame's lowered paint operations, in authoring order.
/// See the module docs for the required/provided split.
pub(crate) trait PaintSink {
    /// Push a clip region. `payload.corners` is zero for a rect clip.
    fn clip(&mut self, payload: PushClipPayload);

    fn pop_clip(&mut self);

    fn push_transform(&mut self, transform: TranslateScale);

    fn pop_transform(&mut self);

    /// One quad-tier draw — rect, windowed rect, shadow, or triangle.
    fn quad(&mut self, payload: DrawQuadPayload);

    fn text(&mut self, payload: DrawTextPayload);

    fn mesh(&mut self, payload: DrawMeshPayload);

    fn polyline(&mut self, payload: DrawPolylinePayload);

    /// `paint` is `Some` exactly when this image composites a `GpuView`,
    /// carrying the app callback the off-screen target is painted with.
    fn image(&mut self, payload: DrawImagePayload, paint: Option<&GpuPaintRef>);

    fn curve(&mut self, payload: DrawCurvePayload);

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
        self.draw_quad(DrawQuadPayload {
            geom: QuadGeom::Rect { rect, corners },
            fill: fill_color,
            stroke: stroke.normalized(),
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
        self.draw_quad(DrawQuadPayload {
            geom: QuadGeom::Rect { rect, corners },
            fill: color,
            // A shadow has no stroke; its whole edge is the blur.
            stroke: ShapeStroke::NONE,
            fill_kind,
            fill_lut_row: LutRow::FALLBACK,
            fill_axis,
        });
    }

    /// The one no-op gate for the quad tier — rect, windowed rect,
    /// shadow, and triangle all funnel through it, so the four cannot
    /// drift apart on what counts as invisible.
    #[inline]
    fn draw_quad(&mut self, payload: DrawQuadPayload) {
        if payload.is_noop() {
            return;
        }
        self.quad(payload);
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

    /// Paint a textured rect. `paint` is `Some` exactly when this
    /// composites a `GpuView`, and carries the callback its off-screen
    /// target is painted with — so the composite and the target it needs
    /// cannot come apart. Deciding `payload.gpu_view` here, rather than
    /// at either construction site, is what keeps the flag and the
    /// callback from disagreeing.
    fn draw_image(&mut self, mut payload: DrawImagePayload, paint: Option<&GpuPaintRef>) {
        payload.gpu_view = paint.is_some();
        if payload.is_noop() {
            return;
        }
        self.image(payload, paint);
    }

    fn draw_curve(&mut self, payload: DrawCurvePayload) {
        if payload.is_noop() {
            return;
        }
        self.curve(payload);
    }

    /// Paint a rounded triangle from three owner-local `points` offset
    /// by `origin`. Same quad-tier draw as [`Self::draw_rect`], down to
    /// the shared stroke normalization — only the geometry and the SDF
    /// the `FillKind` selects differ.
    fn draw_triangle(
        &mut self,
        origin: glam::Vec2,
        points: [glam::Vec2; 3],
        fill: ColorF16,
        radius: f32,
        stroke: ShapeStroke,
    ) {
        let [a, b, c] = points;
        self.draw_quad(DrawQuadPayload {
            geom: QuadGeom::Triangle {
                origin,
                a,
                b,
                c,
                radius,
            },
            fill,
            stroke: stroke.normalized(),
            fill_kind: FillKind::TRIANGLE,
            // The composer overwrites both reused lanes from the
            // transformed points, so neither is read from here.
            fill_lut_row: LutRow::FALLBACK,
            fill_axis: FillAxis::ZERO,
        });
    }

    /// Paint a polyline against already-staged points and colors. The
    /// recorder pushes onto `polyline_points` / `polyline_colors`
    /// directly (so the encoder can apply the owner-rect offset inline
    /// without an intermediate scratch buffer) and passes the resulting
    /// spans in the payload. The `color_mode`-dictated `colors_len` is a
    /// caller invariant enforced upstream by
    /// `PolylineColors::assert_matches` in `Shapes::add`.
    fn draw_polyline(&mut self, payload: DrawPolylinePayload) {
        // Asserted, not gated — the one payload whose no-op conditions
        // are *already guaranteed* when it gets here, so a failure is a
        // broken contract rather than a value to filter.
        //
        // Both conditions are authoring-derived and unchanged by
        // lowering: `PolylineShape::is_noop` rejects `< 2` points and a
        // non-painting width before `Shapes::add` lowers anything, and
        // the encoder forwards the record's span length and width
        // verbatim. The other payloads gate instead of asserting
        // because theirs are layout *outputs* — a rect resolved from
        // the owner's arranged box, a text extent from the shaped
        // measure — which can legitimately collapse to nothing.
        //
        // Debug-only is safe: the composer handles a degenerate polyline
        // by emitting no geometry (pinned by
        // `degenerate_polyline_emits_nothing_rather_than_panicking`), so
        // a release build that somehow reached here still paints
        // correctly — it just doesn't pay two comparisons per polyline
        // per frame to re-establish what upstream already proved.
        debug_assert!(
            !payload.is_noop(),
            "degenerate polyline reached the sink — `PolylineShape::is_noop` \
             should have dropped it: {payload:?}",
        );
        self.polyline(payload);
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::color::{Color, ColorF16};
    use crate::primitives::corners::Corners;
    use crate::primitives::rect::Rect;

    use crate::primitives::fill_wire::FillKind;
    use crate::primitives::stroke::Stroke;
    use crate::primitives::texture_id::TextureId;
    use crate::renderer::frontend::paint_sink::PaintSink;
    use crate::renderer::frontend::payload::{
        BrushSource, DrawImagePayload, DrawPolylinePayload, QuadGeom,
    };
    use crate::renderer::frontend::record_sink::{PaintCall, RecordedPaint};
    use crate::renderer::gpu_view::{GpuFrameCtx, GpuPaint, GpuPaintRef};
    use crate::scene::shapes::paint::ShapeStroke;
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

    /// Every quad-tier draw runs one stroke normalization
    /// ([`ShapeStroke::normalized`]): a noop stroke — transparent
    /// colour, zero width, or a NaN width — lands in the payload as
    /// [`ShapeStroke::NONE`]; anything else passes through verbatim.
    ///
    /// The NaN row is the interesting one. It normalizes away like any
    /// other non-painting width, which is deliberate: catching a NaN
    /// *loudly* is `Shape::debug_assert_no_nan`'s job at the authoring
    /// boundary, so by the time a value reaches here the useful
    /// behaviour is to fail safe — and to do it identically for every
    /// shape, rather than per-path (a rect forwarding NaN to the GPU
    /// while a triangle scrubs it).
    ///
    /// The table pins both halves: the exact normalized stroke per case,
    /// **and** that the rect and triangle paths emit bit-identical
    /// stroke fields — the regression guard against either path growing
    /// its own copy again.
    #[test]
    fn quad_stroke_normalization_is_shared_by_rect_and_triangle() {
        let fill = Color::rgb(1.0, 0.0, 0.0);
        let green = Color::rgb(0.0, 1.0, 0.0);
        let cases: [(&str, ShapeStroke, bool); 4] = [
            (
                "transparent_color",
                Stroke::solid(Color::TRANSPARENT, 3.0).into(),
                true,
            ),
            ("zero_width", Stroke::solid(green, 0.0).into(), true),
            ("nan_width", Stroke::solid(green, f32::NAN).into(), true),
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
            let Some(PaintCall::Quad(rp)) = rb.calls.first() else {
                panic!("case {label}: expected a rect quad");
            };
            assert!(
                matches!(rp.geom, QuadGeom::Rect { .. }),
                "case {label}: draw_rect must emit rect geometry",
            );

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
            let Some(PaintCall::Quad(tp)) = tb.calls.first() else {
                panic!("case {label}: expected a triangle quad");
            };
            assert!(
                matches!(tp.geom, QuadGeom::Triangle { .. }),
                "case {label}: draw_triangle must emit triangle geometry",
            );
            assert_eq!(tp.fill_kind, FillKind::TRIANGLE, "case {label}");

            assert_eq!(tp.stroke.color, rp.stroke.color, "case {label}");
            assert_eq!(
                tp.stroke.width.to_bits(),
                rp.stroke.width.to_bits(),
                "case {label}",
            );
            if expect_normalized {
                assert_eq!(tp.stroke.color, ColorF16::TRANSPARENT, "case {label}");
                assert_eq!(tp.stroke.width, 0.0, "case {label}");
            } else {
                assert_eq!(tp.stroke.color, ColorF16::from(green), "case {label}");
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
            sink.draw_image(
                DrawImagePayload::image(
                    rect,
                    Vec2::ZERO,
                    Vec2::ONE,
                    ColorF16::from(Color::WHITE),
                    handle,
                    0,
                ),
                Some(&paint),
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
}
