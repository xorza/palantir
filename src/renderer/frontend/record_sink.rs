//! Recording [`PaintSink`] for tests and benches.
//!
//! Production paints straight into a `ComposeSession`, which leaves no
//! artifact to assert on. [`RecordedPaint`] captures the same call
//! sequence as owned values so tests can count, match, and compare it,
//! and [`RecordedPaint::replay`] pushes it into any other sink — which
//! is what lets the compose bench measure compose alone, feeding a
//! stream it recorded once outside the timed loop.
//!
//! Recording happens *below* [`PaintSink`]'s `draw_*` gates, so a call
//! only lands here if it survived the no-op gate. Two recordings
//! comparing equal therefore means the two encodes agreed on every
//! painted operation, in order.

// Test-support surface: which parts are live depends on whether the
// build enables `test`, `internals`, or both.
#![allow(dead_code)]

use crate::primitives::transform::TranslateScale;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::{
    DrawCurvePayload, DrawImagePayload, DrawMeshPayload, DrawPolylinePayload, DrawQuadPayload,
    DrawTextPayload, PushClipPayload,
};
use crate::renderer::gpu_view::GpuPaintRef;

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
            },
        }

        impl PaintCall {
            /// Short name for assertion messages — the variant alone,
            /// without the payload a `Debug` dump would print.
            pub(crate) fn kind(&self) -> &'static str {
                match self {
                    $( Self::$variant(_) => stringify!($variant), )*
                    $( Self::$unit => stringify!($unit), )*
                    Self::Image { .. } => "Image",
                }
            }

            /// Push this call back into `sink` through the *required*
            /// half of the trait. See [`RecordedPaint::replay`] for why
            /// that bypasses the no-op gate.
            fn replay_into(&self, sink: &mut impl PaintSink) {
                match self {
                    $( Self::$variant(payload) => sink.$method(*payload), )*
                    $( Self::$unit => sink.$unit_method(), )*
                    Self::Image { payload, paint } => sink.image(*payload, paint.as_ref()),
                }
            }
        }

        impl PaintSink for RecordedPaint {
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
            fn image(&mut self, payload: DrawImagePayload, paint: Option<&GpuPaintRef>) {
                self.calls.push(PaintCall::Image {
                    payload,
                    paint: paint.cloned(),
                });
            }
        }
    };
}

paint_calls! {
    Clip(PushClipPayload) => clip,
    PushTransform(TranslateScale) => push_transform,
    Quad(DrawQuadPayload) => quad,
    Text(DrawTextPayload) => text,
    Mesh(DrawMeshPayload) => mesh,
    Polyline(DrawPolylinePayload) => polyline,
    Curve(DrawCurvePayload) => curve,
    --
    PopClip => pop_clip,
    PopTransform => pop_transform,
}

/// Every paint call one encode made, in order.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RecordedPaint {
    pub(crate) calls: Vec<PaintCall>,
}

impl RecordedPaint {
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

    /// Number of recorded calls matching `pred` — the shape most
    /// encoder assertions want ("how many clips did this subtree emit").
    pub(crate) fn count(&self, pred: impl Fn(&PaintCall) -> bool) -> usize {
        self.calls.iter().filter(|call| pred(call)).count()
    }
}

/// Assert two encodes painted the same sequence, reporting the first
/// divergence by index and kind instead of dumping both call lists.
pub(crate) fn assert_same_paint(left: &RecordedPaint, right: &RecordedPaint) {
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
    }
    assert_eq!(
        left.calls.len(),
        right.calls.len(),
        "paint call counts differ ({} vs {}); the common prefix matched",
        left.calls.len(),
        right.calls.len(),
    );
}
