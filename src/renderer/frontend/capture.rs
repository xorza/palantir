//! Capturing [`PaintSink`] for tests and benches.
//!
//! Production paints straight into a `ComposeSession`, which leaves no
//! artifact to assert on. [`PaintCapture`] holds the same call sequence
//! as owned values so tests can count, match, and compare it, and
//! [`PaintCapture::replay`] pushes it into any other sink — which is
//! what lets the compose bench measure compose alone, feeding a stream
//! it captured once outside the timed loop.
//!
//! *Capture*, not *record*: this crate spends "record" on the authoring
//! pass (`App::record`, `RecordStore`) and on SoA rows (`NodeRecord`),
//! and a third meaning on the same word cost more than the rename did.
//!
//! Capturing happens *below* [`PaintSink`]'s `draw_*` gates, so a call
//! only lands here if it survived the no-op gate. Two captures comparing
//! equal therefore means the two encodes agreed on every painted
//! operation, in order.

use crate::primitives::translate_scale::TranslateScale;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::draw_curve_payload::DrawCurvePayload;
use crate::renderer::frontend::payload::draw_icon_payload::DrawIconPayload;
use crate::renderer::frontend::payload::draw_image_payload::{
    DrawImagePayload, ImageDraw, ViewPaint,
};
use crate::renderer::frontend::payload::draw_mesh_payload::DrawMeshPayload;
use crate::renderer::frontend::payload::draw_polyline_payload::DrawPolylinePayload;
use crate::renderer::frontend::payload::draw_quad_payload::DrawQuadPayload;
use crate::renderer::frontend::payload::draw_text_payload::DrawTextPayload;
use crate::renderer::frontend::payload::push_clip_payload::PushClipPayload;
use crate::renderer::gpu_paint::gpu_paint_ref::GpuPaintRef;

/// Declare [`PaintCall`] alongside the three transcriptions that have
/// to stay in lockstep with it — the variant name used in assertion
/// messages, the replay dispatch, and the [`PaintSink`] impl that does
/// the recording — from one table of `Variant(Payload) => sink_method`.
///
/// Two shapes because the sink has two: calls carrying a single `Copy`
/// payload, and the two pops that carry nothing. `image` is written out
/// by hand below the repetitions — it is the one call whose sink method
/// takes a second argument, and pretending it were uniform would cost
/// more than it saves.
macro_rules! paint_calls {
    (
        $( $variant:ident($payload:ty) => $method:ident, )*
        --
        $( $unit:ident => $unit_method:ident, )*
    ) => {
        /// One recorded [`PaintSink`] call, owning whatever the call
        /// carried. A `GpuView` composite records as [`Self::Image`]
        /// carrying its paint callback, exactly as the sink sees it.
        #[derive(Clone, Debug, PartialEq)]
        pub(crate) enum PaintCall {
            $( $variant($payload), )*
            $( $unit, )*
            Image {
                payload: DrawImagePayload,
                paint: Option<GpuPaintRef>,
                epoch: u64,
            },
        }

        impl PaintCall {
            /// Short name for assertion messages — the variant alone,
            /// without the payload a `Debug` dump would print.
            #[cfg(test)]
            pub(crate) fn kind(&self) -> &'static str {
                match self {
                    $( Self::$variant(_) => stringify!($variant), )*
                    $( Self::$unit => stringify!($unit), )*
                    Self::Image { .. } => "Image",
                }
            }

            /// Push this call back into `sink` through the *required*
            /// half of the trait. See [`PaintCapture::replay`] for why
            /// that bypasses the no-op gate.
            fn replay_into(&self, sink: &mut impl PaintSink) {
                match self {
                    $( Self::$variant(payload) => sink.$method(*payload), )*
                    $( Self::$unit => sink.$unit_method(), )*
                    Self::Image { payload, paint, epoch } => sink.image(ImageDraw {
                        payload: *payload,
                        view: paint.as_ref().map(|paint| ViewPaint { paint, epoch: *epoch }),
                    }),
                }
            }
        }

        impl PaintSink for PaintCapture {
            $(
                fn $method(&mut self, payload: $payload) {
                    self.calls.push(PaintCall::$variant(payload));
                }
            )*
            $(
                fn $unit_method(&mut self) {
                    self.calls.push(PaintCall::$unit);
                }
            )*
            fn image(&mut self, draw: ImageDraw<'_>) {
                self.calls.push(PaintCall::Image {
                    payload: draw.payload,
                    paint: draw.view.map(|view| view.paint.clone()),
                    epoch: draw.view.map_or(0, |view| view.epoch),
                });
            }
        }
    };
}

paint_calls! {
    PushClip(PushClipPayload) => push_clip,
    PushTransform(TranslateScale) => push_transform,
    Quad(DrawQuadPayload) => quad,
    Text(DrawTextPayload) => text,
    Mesh(DrawMeshPayload) => mesh,
    Polyline(DrawPolylinePayload) => polyline,
    Icon(DrawIconPayload) => icon,
    Curve(DrawCurvePayload) => curve,
    --
    PopClip => pop_clip,
    PopTransform => pop_transform,
}

/// Every paint call one encode made, in order.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PaintCapture {
    pub(crate) calls: Vec<PaintCall>,
}

impl PaintCapture {
    /// Push the recorded sequence into `sink`. Calls re-enter through
    /// the *required* half, below the no-op gate — deliberately, so a
    /// replay reproduces the recorded stream exactly rather than
    /// re-filtering it. Sound because every recorded call already
    /// passed the gate once, at record time. Replaying calls that did
    /// **not** come from a recording would bypass it.
    pub(crate) fn replay(&self, sink: &mut impl PaintSink) {
        for call in &self.calls {
            call.replay_into(sink);
        }
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use crate::renderer::frontend::capture::{PaintCall, PaintCapture};
    use crate::renderer::frontend::payload::draw_curve_payload::DrawCurvePayload;
    use crate::renderer::frontend::payload::draw_image_payload::DrawImagePayload;
    use crate::renderer::frontend::payload::draw_text_payload::DrawTextPayload;
    use crate::renderer::frontend::payload::push_clip_payload::PushClipPayload;

    /// Typed reads of one call's payload, for a test that has already pinned
    /// the sequence with [`PaintCapture::kinds`] and wants what a call
    /// carried.
    impl PaintCall {
        pub(crate) fn as_push_clip(&self) -> Option<&PushClipPayload> {
            match self {
                Self::PushClip(payload) => Some(payload),
                _ => None,
            }
        }

        pub(crate) fn as_text(&self) -> Option<&DrawTextPayload> {
            match self {
                Self::Text(payload) => Some(payload),
                _ => None,
            }
        }

        pub(crate) fn as_curve(&self) -> Option<&DrawCurvePayload> {
            match self {
                Self::Curve(payload) => Some(payload),
                _ => None,
            }
        }

        pub(crate) fn as_image(&self) -> Option<&DrawImagePayload> {
            match self {
                Self::Image { payload, .. } => Some(payload),
                _ => None,
            }
        }
    }

    impl PaintCapture {
        /// The recorded calls' kinds, in order — what an assertion about
        /// nesting or order compares, where a count of each kind would let
        /// two siblings pass for a parent and its child.
        pub(crate) fn kinds(&self) -> Vec<&'static str> {
            self.calls.iter().map(PaintCall::kind).collect()
        }
    }

    /// Assert two encodes painted the same sequence, reporting the first
    /// divergence by index and kind instead of dumping both call lists.
    pub(crate) fn assert_same_capture(left: &PaintCapture, right: &PaintCapture) {
        for (i, (l, r)) in left.calls.iter().zip(&right.calls).enumerate() {
            // Compare rendered `Debug`, not `PartialEq`: the payloads are
            // full of `f32`, and derived equality gets both float edge cases
            // wrong here. `NaN != NaN` would fail two byte-identical frames
            // (a NaN stroke width is a documented pass-through, not a noop),
            // and `-0.0 == 0.0` would hide a sign-of-zero drift between
            // them. `Debug` distinguishes signed zeros and prints `NaN` for
            // every NaN, so it is the bitwise-shaped comparison this check
            // wants — no fast path, since the `PartialEq` one would
            // reintroduce the signed-zero hole.
            let (ls, rs) = (format!("{l:?}"), format!("{r:?}"));
            assert!(
                ls == rs,
                "paint call {i} differs: {} vs {}\n  left:  {ls}\n  right: {rs}",
                l.kind(),
                r.kind(),
            );
            // A view's callback prints as a constant, so `Debug` cannot tell
            // two of them apart; identity is what decides.
            if let (PaintCall::Image { paint: lp, .. }, PaintCall::Image { paint: rp, .. }) = (l, r)
            {
                assert!(
                    lp == rp,
                    "paint call {i}: the image draws name different view callbacks"
                );
            }
        }
        assert_eq!(
            left.calls.len(),
            right.calls.len(),
            "paint call counts differ ({} vs {}); the common prefix matched",
            left.calls.len(),
            right.calls.len(),
        );
    }
}
