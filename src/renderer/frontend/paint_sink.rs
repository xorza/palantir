//! The encoder's output surface.
//!
//! [`PaintSink`] is the one interface the [`Encoder`] paints through.
//! In production the only sink is `ComposeSession`, which composes each
//! call straight into a `RenderBuffer` — there is no intermediate
//! command stream. Tests and benches add a capturing sink
//! (`capture`) that holds the same calls as owned values.
//!
//! Each `draw_*` method is the no-op gate for one payload kind: it
//! tests `is_noop` and calls the matching sink method, or nothing.
//! Keeping the gate here rather than in either sink is what keeps it
//! single-copy — it can't drift between the two because there is only
//! one copy of it.
//!
//! Building the payloads is *not* here. A rect's brush lowering, a
//! shadow's fill lanes, a triangle's geometry — those are constructors
//! on the payload types (`DrawQuadPayload::rect`,
//! `PushClipPayload::rect`, …), because none of them touch a sink. The
//! trait is one job: receive a paint op, gated. That is also what keeps
//! it object-safe, so the encoder paints through `&mut dyn PaintSink`
//! and compiles once rather than once per sink.
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
//!    trait's `draw_*` gates) are the **single correctness gate**.
//!    Callers don't pre-check and the encoder doesn't gate per branch;
//!    everything funnels here.
//!
//! So tier 2 is an optimization and tier 3 is correctness — a shape
//! that slips past tier 2 still paints nothing, but pays for lowering.
//! The gate is not *unbypassable*: the ungated half is crate-visible,
//! so `sink.quad(payload)` compiles anywhere and skips it.
//! `PaintCapture::replay` is the one place that does, and only because
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

use crate::primitives::color::ColorF16;
use crate::primitives::rect::Rect;
use crate::primitives::translate_scale::TranslateScale;
use crate::renderer::frontend::payload::draw_curve_payload::DrawCurvePayload;
use crate::renderer::frontend::payload::draw_icon_payload::DrawIconPayload;
use crate::renderer::frontend::payload::draw_image_payload::DrawImagePayload;
use crate::renderer::frontend::payload::draw_mesh_payload::DrawMeshPayload;
use crate::renderer::frontend::payload::draw_polyline_payload::DrawPolylinePayload;
use crate::renderer::frontend::payload::draw_quad_payload::DrawQuadPayload;
use crate::renderer::frontend::payload::draw_text_payload::DrawTextPayload;
use crate::renderer::frontend::payload::push_clip_payload::PushClipPayload;
use crate::renderer::gpu_paint::gpu_paint_ref::GpuPaintRef;
use crate::text::shaped_ref::ShapedTextRef;

/// Sink for one frame's lowered paint operations, in authoring order.
/// One `draw_*` gate and one ungated method per payload kind; see the
/// module docs for why the gate lives here and payload construction
/// doesn't.
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

    fn icon(&mut self, payload: DrawIconPayload);

    fn curve(&mut self, payload: DrawCurvePayload);

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
    /// cannot come apart. That one argument is also what the no-op gate
    /// reads, so the fact travels on one channel rather than being
    /// mirrored onto the payload.
    fn draw_image(&mut self, payload: DrawImagePayload, paint: Option<&GpuPaintRef>) {
        if payload.is_noop(paint.is_some()) {
            return;
        }
        self.image(payload, paint);
    }

    /// Paint a baked icon. Nothing is rasterized here — the sink records
    /// which icon at which logical rect, and the backend resolves that to
    /// pixels once the physical size is known.
    fn draw_icon(&mut self, payload: DrawIconPayload) {
        if payload.is_noop() {
            return;
        }
        self.icon(payload);
    }

    fn draw_curve(&mut self, payload: DrawCurvePayload) {
        if payload.is_noop() {
            return;
        }
        self.curve(payload);
    }

    /// Paint a polyline against already-staged points and colors. The
    /// recorder pushes onto `polyline_points` / `polyline_colors`
    /// directly (so the encoder can apply the owner-rect offset inline
    /// without an intermediate scratch buffer) and passes the resulting
    /// spans in the payload. The `color_mode`-dictated `colors_len` is a
    /// caller invariant checked upstream by
    /// `PolylineColors::debug_assert_matches` in `lower::polyline`.
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
    use crate::primitives::rect::Rect;
    use crate::primitives::texture_id::TextureId;
    use crate::renderer::frontend::capture::{PaintCall, PaintCapture};
    use crate::renderer::frontend::paint_sink::PaintSink;
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
}
