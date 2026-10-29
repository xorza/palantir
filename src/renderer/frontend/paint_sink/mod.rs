//! The encoder's output surface.
//!
//! [`PaintSink`] is the one interface the [`Encoder`] paints through. In
//! production the only sink is `ComposeSession`, which composes each call
//! straight into a `RenderBuffer`; tests and benches add a capturing sink
//! (`capture`).
//!
//! A sink implements the raw half, one method per payload kind. The encoder
//! calls the provided [`PaintSink::draw`] gate, which fades a [`PaintPayload`],
//! tests `is_noop` and forwards or does nothing, so there is one gate for every
//! kind. Payload construction lives on the payload types
//! (`DrawQuadPayload::rect`, ...), not here.
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
//!    `Chrome::is_noop` at `Tree::open_node`) skip lowering work, which a
//!    payload-level gate cannot, since the work has already happened.
//! 3. **Lowered payloads** ([`PaintPayload::is_noop`], called from the
//!    [`PaintSink::draw`] gate) are the single correctness gate; callers do not
//!    pre-check.
//!
//! Tier 2 is an optimization, tier 3 correctness. The gate is bypassable
//! (`sink.quad(payload)` compiles anywhere); `PaintCapture::replay` does so
//! because its input already passed.
//!
//! Exception: a polyline asserts its geometry before gating
//! ([`PaintPayload::check`]). `Shape::Polyline::is_noop` catches those conditions at tier 2 and
//! nothing between the tiers can invalidate them, so a degenerate polyline
//! here is a broken contract. The gate still drops the payload's own no-ops.
//!
//! [`Encoder`]: crate::renderer::frontend::encoder::Encoder

use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::renderer::frontend::payload::draw_curve_payload::DrawCurvePayload;
use crate::renderer::frontend::payload::draw_icon_payload::DrawIconPayload;
use crate::renderer::frontend::payload::draw_image_payload::ImageDraw;
use crate::renderer::frontend::payload::draw_mesh_payload::DrawMeshPayload;
use crate::renderer::frontend::payload::draw_pane_payload::DrawPanePayload;
use crate::renderer::frontend::payload::draw_polyline_payload::DrawPolylinePayload;
use crate::renderer::frontend::payload::draw_quad_payload::DrawQuadPayload;
use crate::renderer::frontend::payload::draw_text_payload::DrawTextPayload;
use crate::renderer::frontend::payload::push_clip_payload::PushClipPayload;

/// Sink for one frame's lowered paint operations, in authoring order. The
/// required methods are what a sink implements; the provided [`Self::draw`]
/// gate is what the encoder paints through, and no sink overrides it.
pub(crate) trait PaintSink {
    /// Push a clip region. `payload.corners` is zero for a rect clip.
    fn push_clip(&mut self, payload: PushClipPayload);

    fn pop_clip(&mut self);

    fn push_transform(&mut self, transform: TranslateScale);

    fn pop_transform(&mut self);

    /// One quad-tier draw (rect, windowed rect, shadow or triangle), all
    /// through one gate so they agree on what is invisible.
    fn quad(&mut self, payload: DrawQuadPayload);

    /// A backdrop pane: a quad over the pixels painted before it, and its key.
    fn pane(&mut self, payload: DrawPanePayload);

    fn text(&mut self, payload: DrawTextPayload);

    /// Paint a mesh against vertices and indices already staged in
    /// `RecordStore.meshes`.
    fn mesh(&mut self, payload: DrawMeshPayload);

    /// Paint a polyline against already-staged points and colors. The
    /// `color_mode`-dictated `colors.len` is checked upstream by
    /// `PolylineShape::per_point` and `PolylineShape::per_segment`.
    fn polyline(&mut self, payload: DrawPolylinePayload);

    /// Paint a textured rect, with the `GpuView` callback when this
    /// composites one; see [`ImageDraw`].
    fn image(&mut self, draw: ImageDraw<'_>);

    /// Paint a baked icon: the sink records which icon at which logical rect,
    /// and the backend rasterizes it once the physical size is known.
    fn icon(&mut self, payload: DrawIconPayload);

    fn curve(&mut self, payload: DrawCurvePayload);

    /// The gate the encoder paints through: fade `payload` by `alpha` (a paint animation's
    /// opacity, `1.0` when still), drop it where it paints nothing, else hand it to the method of
    /// its kind. A parameter, so an encoder arm cannot forget to fade.
    #[inline]
    fn draw(&mut self, payload: impl PaintPayload, alpha: f32)
    where
        Self: Sized,
    {
        payload.check();
        let payload = payload.faded(alpha);
        if payload.is_noop() {
            return;
        }
        payload.paint(self);
    }
}

/// A lowered draw: it fades, says whether it paints, and names the [`PaintSink`] method that
/// takes it, so one gate ([`PaintSink::draw`]) serves every kind.
pub(crate) trait PaintPayload: Sized {
    /// This draw with its alpha scaled by `by`.
    fn faded(self, by: f32) -> Self;

    /// Whether it paints nothing, the one correctness gate (module docs, tier 3).
    fn is_noop(&self) -> bool;

    /// Hand it to the method of `sink` that takes its kind.
    fn paint(self, sink: &mut impl PaintSink);

    /// Assert what upstream guarantees of it, ahead of the gate; nothing by default.
    fn check(&self) {}
}

#[cfg(test)]
mod tests;
