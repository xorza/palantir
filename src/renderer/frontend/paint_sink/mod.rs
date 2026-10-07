//! The encoder's output surface.
//!
//! [`PaintSink`] is the one interface the [`Encoder`] paints through. In
//! production the only sink is `ComposeSession`, which composes each call
//! straight into a `RenderBuffer`; tests and benches add a capturing sink
//! (`capture`).
//!
//! A sink implements the raw half, one method per payload kind. The encoder
//! calls the provided `draw_*` gates, which test `is_noop` and forward or do
//! nothing, so there is one copy of each gate. Payload construction lives on
//! the payload types (`DrawQuadPayload::rect`, ...), not here.
//!
//! The encoder is generic over the sink, not `&mut dyn PaintSink`, so the
//! gates inline.
//!
//! ## Noop policy
//!
//! The canonical statement of the pipeline's tier policy; other tiers point
//! here and document only which values they consider invisible.
//!
//! 1. **Primitives** (`RgbaF32`, `Stroke`, `Shadow`, `Brush`, ...) answer
//!    whether a value is invisible.
//! 2. **Authoring shapes** (`Shape::is_noop` at `Shapes::add`;
//!    `Background::is_noop` at `Tree::open_node`) skip lowering work, which a
//!    payload-level gate cannot, since the work has already happened.
//! 3. **Lowered payloads** (`Draw*Payload::is_noop`, called from the `draw_*`
//!    gates) are the single correctness gate; callers do not pre-check.
//!
//! Tier 2 is an optimization, tier 3 correctness. The gate is bypassable
//! (`sink.quad(payload)` compiles anywhere); `PaintCapture::replay` does so
//! because its input already passed.
//!
//! Exception: [`PaintSink::draw_polyline`] asserts its geometry before
//! gating. `Shape::Polyline::is_noop` catches those conditions at tier 2 and
//! nothing between the tiers can invalidate them, so a degenerate polyline
//! here is a broken contract. The gate still drops the payload's own no-ops.
//! [`Encoder`]: crate::renderer::frontend::encoder::Encoder

use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::renderer::frontend::payload::draw_curve_payload::DrawCurvePayload;
use crate::renderer::frontend::payload::draw_icon_payload::DrawIconPayload;
use crate::renderer::frontend::payload::draw_image_payload::ImageDraw;
use crate::renderer::frontend::payload::draw_mesh_payload::DrawMeshPayload;
use crate::renderer::frontend::payload::draw_polyline_payload::DrawPolylinePayload;
use crate::renderer::frontend::payload::draw_quad_payload::DrawQuadPayload;
use crate::renderer::frontend::payload::draw_text_payload::DrawTextPayload;
use crate::renderer::frontend::payload::push_clip_payload::PushClipPayload;

/// Sink for one frame's lowered paint operations, in authoring order. The
/// required methods are what a sink implements; the provided `draw_*` gates
/// are what the encoder paints through, and no sink overrides them.
///
/// Each gate takes an `alpha` (a paint animation's opacity) and folds it into
/// the payload before testing it, so a shape animated to nothing drops out
/// through the existing gate. It is a parameter so an encoder arm cannot
/// forget to fade. `1.0` means no animation.
pub(crate) trait PaintSink {
    /// Push a clip region. `payload.corners` is zero for a rect clip.
    fn push_clip(&mut self, payload: PushClipPayload);

    fn pop_clip(&mut self);

    fn push_transform(&mut self, transform: TranslateScale);

    fn pop_transform(&mut self);

    /// One quad-tier draw (rect, windowed rect, shadow or triangle), all
    /// through one gate so they agree on what is invisible.
    fn quad(&mut self, payload: DrawQuadPayload);

    fn text(&mut self, payload: DrawTextPayload);

    /// Paint a mesh against vertices and indices already staged in
    /// `RecordStore.meshes`.
    fn mesh(&mut self, payload: DrawMeshPayload);

    /// Paint a polyline against already-staged points and colors. The
    /// `color_mode`-dictated `colors_len` is checked upstream by
    /// `PolylineColors::assert_matches` in `lower::polyline`.
    fn polyline(&mut self, payload: DrawPolylinePayload);

    /// Paint a textured rect, with the `GpuView` callback when this
    /// composites one; see [`ImageDraw`].
    fn image(&mut self, draw: ImageDraw<'_>);

    /// Paint a baked icon: the sink records which icon at which logical rect,
    /// and the backend rasterizes it once the physical size is known.
    fn icon(&mut self, payload: DrawIconPayload);

    fn curve(&mut self, payload: DrawCurvePayload);

    #[inline]
    fn draw_quad(&mut self, payload: DrawQuadPayload, alpha: f32) {
        let payload = payload.faded(alpha);
        if payload.is_noop() {
            return;
        }
        self.quad(payload);
    }

    #[inline]
    fn draw_text(&mut self, payload: DrawTextPayload, alpha: f32) {
        let payload = payload.faded(alpha);
        if payload.is_noop() {
            return;
        }
        self.text(payload);
    }

    #[inline]
    fn draw_mesh(&mut self, payload: DrawMeshPayload, alpha: f32) {
        let payload = payload.faded(alpha);
        if payload.is_noop() {
            return;
        }
        self.mesh(payload);
    }

    #[inline]
    fn draw_icon(&mut self, payload: DrawIconPayload, alpha: f32) {
        let payload = payload.faded(alpha);
        if payload.is_noop() {
            return;
        }
        self.icon(payload);
    }

    #[inline]
    fn draw_curve(&mut self, payload: DrawCurvePayload, alpha: f32) {
        let payload = payload.faded(alpha);
        if payload.is_noop() {
            return;
        }
        self.curve(payload);
    }

    #[inline]
    fn draw_image(&mut self, payload: ImageDraw<'_>, alpha: f32) {
        let payload = payload.faded(alpha);
        if payload.is_noop() {
            return;
        }
        self.image(payload);
    }

    #[inline]
    fn draw_polyline(&mut self, payload: DrawPolylinePayload, alpha: f32) {
        let payload = payload.faded(alpha);
        // Geometry is asserted, not gated: `PolylineShape::is_noop` rejects
        // `< 2` points and a non-painting width before lowering, and the
        // encoder forwards both verbatim. Other payloads gate because theirs
        // are layout outputs that can legitimately collapse. Debug-only is
        // safe: `is_noop` below also covers the geometry, and the composer
        // emits nothing for one. The assert names the broken contract.
        debug_assert!(
            !payload.is_degenerate(),
            "degenerate polyline reached the sink — `PolylineShape::is_noop` \
             should have dropped it: {payload:?}",
        );
        if payload.is_noop() {
            return;
        }
        self.polyline(payload);
    }
}

#[cfg(test)]
mod tests;
