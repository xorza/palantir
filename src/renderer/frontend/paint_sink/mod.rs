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
//! (`DrawClosedPayload::new`, ...), not here.
//!
//! The encoder is generic over the sink, not `&mut dyn PaintSink`, so the
//! gates inline.
//!
//! ## The payloads are a seam
//!
//! The encoder lowers each shape to a `Draw*Payload` and paints it through
//! [`PaintSink`], not straight into the `RenderBuffer`, so the composer can be
//! driven alone: `PaintCapture` records one frame's stream once, and the
//! composer's tests and benches replay it against the composer with no scene,
//! layout or encoder in front. Refused: lowering into the rows directly, as
//! egui's tessellator writes its meshes and vello's encoding its scene, which
//! makes every composer test build a scene and every composer bench time the
//! encoder too. Cost: the payload types, and the sink the encoder is generic
//! over, which production monomorphizes once.
//!
//! ## Noop policy
//!
//! The canonical statement of the pipeline's tier policy; other tiers point
//! here and document only which values they consider invisible.
//!
//! 1. **Primitives** (`RgbaF32`, `Border`, `Shadow`, `Brush`, ...) answer
//!    whether a value is invisible.
//! 2. **Authoring shapes** (a draw that paints nothing at `RecordStore::record`;
//!    `Chrome::is_noop` at `Tree::open_node`) skip lowering work, which a
//!    payload-level gate cannot, since the work has already happened.
//! 3. **Lowered payloads** ([`PaintPayload::is_noop`], called from the
//!    [`PaintSink::draw`] gate) are the single correctness gate; callers do not
//!    pre-check.
//!
//! Tier 2 is an optimization, tier 3 correctness. The gate is bypassable
//! (`sink.fill(payload)` compiles anywhere); `PaintCapture::replay` does so
//! because its input already passed.
//!
//! Exception: a polyline asserts its geometry before gating
//! ([`PaintPayload::check`]). `RecordStore::record` drops those conditions at tier 2 and
//! nothing between the tiers can invalidate them, so a degenerate polyline
//! here is a broken contract. The gate still drops the payload's own no-ops.
//!
//! [`Encoder`]: crate::renderer::frontend::encoder::Encoder

use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::renderer::frontend::payload::draw_closed_payload::DrawClosedPayload;
use crate::renderer::frontend::payload::draw_closed_payload::closed_effect::ClosedEffect;
use crate::renderer::frontend::payload::draw_closed_payload::closed_fill::ClosedFill;
use crate::renderer::frontend::payload::draw_closed_payload::closed_image::ClosedImage;
use crate::renderer::frontend::payload::draw_icon_payload::DrawIconPayload;
use crate::renderer::frontend::payload::draw_mesh_payload::DrawMeshPayload;
use crate::renderer::frontend::payload::draw_shadow_payload::DrawShadowPayload;
use crate::renderer::frontend::payload::draw_stroke_payload::DrawStrokePayload;
use crate::renderer::frontend::payload::draw_text_payload::DrawTextPayload;
use crate::renderer::frontend::payload::push_clip_payload::PushClipPayload;
use crate::renderer::frontend::payload::spin::Spin;

/// Sink for one frame's lowered paint operations, in authoring order. The
/// required methods are what a sink implements; the provided [`Self::draw`]
/// gate is what the encoder paints through, and no sink overrides it.
pub(crate) trait PaintSink {
    /// Push a clip region. `payload.corners` is zero for a rect clip.
    fn push_clip(&mut self, payload: PushClipPayload);

    fn pop_clip(&mut self);

    fn push_transform(&mut self, transform: TranslateScale);

    fn pop_transform(&mut self);

    /// Paint a fill and a border over a box or a triangle.
    fn fill(&mut self, payload: DrawClosedPayload<ClosedFill>);

    /// Paint a box's drop or inset shadow.
    fn shadow(&mut self, payload: DrawShadowPayload);

    fn text(&mut self, payload: DrawTextPayload);

    /// Paint a mesh against vertices and indices already staged in
    /// `RecordStore.mesh_vertices` and `RecordStore.mesh_idxs`.
    fn mesh(&mut self, payload: DrawMeshPayload);

    /// Paint a stroke along a cubic, an arc or a polyline; a polyline's points and colors are
    /// already staged, their counts checked upstream by `PolylineShape::point_colors` and
    /// `PolylineShape::segment_colors`.
    fn stroke(&mut self, payload: DrawStrokePayload);

    /// Paint an image over a box or a triangle, and the `GpuView` it composites, if any.
    fn image(&mut self, payload: DrawClosedPayload<ClosedImage>);

    /// Paint a baked icon: the sink records which icon at which logical rect,
    /// and the backend rasterizes it once the physical size is known.
    fn icon(&mut self, payload: DrawIconPayload);

    /// Paint an effect over a box or a triangle: an app's paint, or a box's material.
    fn effect(&mut self, payload: DrawClosedPayload<ClosedEffect>);

    /// The gate the encoder paints through: fade and turn `payload` by `animation`, a paint
    /// animation's opacity and turn this frame, drop it where it paints nothing, else hand it to
    /// the method of its kind. A parameter, so an encoder arm cannot forget to fade or to turn.
    /// An `f32` is a fade alone.
    #[inline]
    fn draw(&mut self, payload: impl PaintPayload, animation: impl Into<DrawAnimation>)
    where
        Self: Sized,
    {
        let DrawAnimation { alpha, spin } = animation.into();
        payload.check();
        let payload = payload.faded(alpha).turned(spin);
        if payload.is_noop() {
            return;
        }
        payload.paint(self);
    }
}

/// What a paint animation does to one draw this frame, as the gate ([`PaintSink::draw`]) applies
/// it: its opacity, `1.0` when still, and its turn, so no encoder arm can forget either; an `f32`
/// is a fade alone. Refused: a turn passed by hand in each arm; `PaintMod`, the cursor's sample,
/// which holds the rotation and the time, not the pivot a draw turns about.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawAnimation {
    pub(crate) alpha: f32,
    pub(crate) spin: Option<Spin>,
}

impl DrawAnimation {
    /// No fade and no turn.
    pub(crate) const STILL: Self = Self {
        alpha: 1.0,
        spin: None,
    };
}

/// A fade alone, as a draw no paint animation turns takes it.
impl From<f32> for DrawAnimation {
    fn from(alpha: f32) -> Self {
        Self { alpha, spin: None }
    }
}

/// A lowered draw: it fades, turns, says whether it paints, and names the [`PaintSink`] method
/// that takes it, so one gate ([`PaintSink::draw`]) serves every kind.
pub(crate) trait PaintPayload: Sized {
    /// This draw with its alpha scaled by `by`.
    fn faded(self, by: f32) -> Self;

    /// The draw's turn, which [`Self::turned`] sets.
    fn spin_mut(&mut self) -> &mut Option<Spin>;

    /// This draw turned by `spin`; unchanged by `None`. A draw turns once.
    #[inline]
    fn turned(mut self, spin: Option<Spin>) -> Self {
        if let Some(spin) = spin {
            let slot = self.spin_mut();
            debug_assert!(slot.is_none(), "a draw turns once: {slot:?}");
            *slot = Some(spin);
        }
        self
    }

    /// Whether it paints nothing, the one correctness gate (module docs, tier 3).
    fn is_noop(&self) -> bool;

    /// Hand it to the method of `sink` that takes its kind.
    fn paint(self, sink: &mut impl PaintSink);

    /// Assert what upstream guarantees of it, ahead of the gate; nothing by default.
    fn check(&self) {}
}

#[cfg(test)]
mod tests;
