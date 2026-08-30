//! The encoder's output surface.
//!
//! [`PaintSink`] is the one interface the [`Encoder`] paints through.
//! In production the only sink is `ComposeSession`, which composes each
//! call straight into a `RenderBuffer` — there is no intermediate
//! command stream. Tests and benches add a capturing sink
//! (`capture`) that holds the same calls as owned values.
//!
//! One trait, two halves. A sink *implements* the raw half: one method
//! per payload kind, and nothing else. The encoder *calls* the provided
//! half: one `draw_*` no-op gate per kind, which tests `is_noop` and
//! forwards to the raw method or does nothing. The gates are provided
//! methods, so there is one copy of each and no sink writes a second.
//!
//! Building the payloads is *not* here. A rect's brush lowering, a
//! shadow's fill lanes, a triangle's geometry — those are constructors
//! on the payload types (`DrawQuadPayload::rect`,
//! `PushClipPayload::rect`, …), because none of them touch a sink.
//! `PaintSink` has one job: receive a paint op.
//!
//! The encoder is generic over the sink rather than painting through
//! `&mut dyn PaintSink`: production has one sink, so every draw of
//! every shape was an indirect call to a statically known target, and
//! the gates could not inline through it.
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
//! The gate is not *unbypassable*: `PaintSink` is crate-visible, so
//! `sink.quad(payload)` compiles anywhere and skips it.
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

use crate::primitives::translate_scale::TranslateScale;
use crate::renderer::frontend::payload::draw_curve_payload::DrawCurvePayload;
use crate::renderer::frontend::payload::draw_icon_payload::DrawIconPayload;
use crate::renderer::frontend::payload::draw_image_payload::ImageDraw;
use crate::renderer::frontend::payload::draw_mesh_payload::DrawMeshPayload;
use crate::renderer::frontend::payload::draw_polyline_payload::DrawPolylinePayload;
use crate::renderer::frontend::payload::draw_quad_payload::DrawQuadPayload;
use crate::renderer::frontend::payload::draw_text_payload::DrawTextPayload;
use crate::renderer::frontend::payload::push_clip_payload::PushClipPayload;

macro_rules! noop_gates {
    ($( $gate:ident($payload:ty) => $method:ident, )*) => {
        $(
            #[inline]
            fn $gate(&mut self, payload: $payload) {
                if payload.is_noop() {
                    return;
                }
                self.$method(payload);
            }
        )*
    };
}

/// Sink for one frame's lowered paint operations, in authoring order.
///
/// The required methods are exactly the calls a sink implements. The
/// provided `draw_*` methods below are the no-op gates the encoder
/// paints through, and no sink overrides them.
pub(crate) trait PaintSink {
    /// Push a clip region. `payload.corners` is zero for a rect clip.
    fn push_clip(&mut self, payload: PushClipPayload);

    fn pop_clip(&mut self);

    fn push_transform(&mut self, transform: TranslateScale);

    fn pop_transform(&mut self);

    /// One quad-tier draw — rect, windowed rect, shadow, or triangle.
    /// All four funnel through the one gate below, so they cannot drift
    /// apart on what counts as invisible.
    fn quad(&mut self, payload: DrawQuadPayload);

    fn text(&mut self, payload: DrawTextPayload);

    /// Paint a mesh against already-staged vertices + indices in
    /// `RecordPayloads.meshes`. The recorder pushes verts (translated
    /// into the owner's logical-px world coords) and indices directly,
    /// so the encoder applies the owner-rect offset inline without an
    /// intermediate scratch buffer.
    fn mesh(&mut self, payload: DrawMeshPayload);

    /// Paint a polyline against already-staged points and colors, on the
    /// same terms as [`Self::mesh`]. The `color_mode`-dictated
    /// `colors_len` is a caller invariant checked upstream by
    /// `PolylineColors::assert_matches` in `lower::polyline`.
    fn polyline(&mut self, payload: DrawPolylinePayload);

    /// Paint a textured rect, with the `GpuView` callback beside it when
    /// this composites one — see [`ImageDraw`].
    fn image(&mut self, draw: ImageDraw<'_>);

    /// Paint a baked icon. Nothing is rasterized here — the sink records
    /// which icon at which logical rect, and the backend resolves that to
    /// pixels once the physical size is known.
    fn icon(&mut self, payload: DrawIconPayload);

    fn curve(&mut self, payload: DrawCurvePayload);

    // One gate per payload kind whose whole body is "drop it if it
    // paints nothing". Written once rather than six times: what differs
    // between them is the payload type and the sink method, which is all
    // the table says.
    noop_gates! {
        draw_quad(DrawQuadPayload) => quad,
        draw_text(DrawTextPayload) => text,
        draw_mesh(DrawMeshPayload) => mesh,
        draw_icon(DrawIconPayload) => icon,
        draw_curve(DrawCurvePayload) => curve,
        draw_image(ImageDraw<'_>) => image,
    }

    #[inline]
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
mod tests;
