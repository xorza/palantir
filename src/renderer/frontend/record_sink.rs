//! Recording [`PaintSink`] for tests and benches.
//!
//! Production paints straight into a `ComposeSession`, which leaves no
//! artifact to assert on. [`RecordedPaint`] captures the same call
//! sequence as owned values so tests can count, match, and compare it,
//! and [`RecordedPaint::replay`] pushes it into any other sink — which
//! is what lets the compose bench measure compose alone, feeding a
//! stream it recorded once outside the timed loop.
//!
//! Recording happens *below* [`PaintSink`]'s provided half, so a call
//! only lands here if it survived the no-op gate. Two recordings
//! comparing equal therefore means the two encodes agreed on every
//! painted operation, in order.

// Test-support surface: which parts are live depends on whether the
// build enables `test`, `internals`, or both.
#![allow(dead_code)]

use crate::primitives::transform::TranslateScale;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::{
    DrawArcPayload, DrawCurvePayload, DrawImagePayload, DrawMeshPayload, DrawPolylinePayload,
    DrawRectPayload, DrawShadowPayload, DrawTextPayload, DrawTrianglePayload, PushClipPayload,
};
use crate::renderer::gpu_view::GpuPaintRef;

/// One recorded [`PaintSink`] call, owning whatever the call carried.
/// A `GpuView` composite records as [`Self::Image`] carrying its paint
/// callback, exactly as the sink sees it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PaintCall {
    Clip(PushClipPayload),
    PopClip,
    PushTransform(TranslateScale),
    PopTransform,
    Rect(DrawRectPayload),
    Shadow(DrawShadowPayload),
    Text(DrawTextPayload),
    Mesh(DrawMeshPayload),
    Polyline(DrawPolylinePayload),
    Image {
        payload: DrawImagePayload,
        paint: Option<GpuPaintRef>,
    },
    Curve(DrawCurvePayload),
    Arc(DrawArcPayload),
    Triangle(DrawTrianglePayload),
}

impl PaintCall {
    /// Short name for assertion messages — the variant alone, without
    /// the payload a `Debug` dump would print.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::Clip(_) => "Clip",
            Self::PopClip => "PopClip",
            Self::PushTransform(_) => "PushTransform",
            Self::PopTransform => "PopTransform",
            Self::Rect(_) => "Rect",
            Self::Shadow(_) => "Shadow",
            Self::Text(_) => "Text",
            Self::Mesh(_) => "Mesh",
            Self::Polyline(_) => "Polyline",
            Self::Image { .. } => "Image",
            Self::Curve(_) => "Curve",
            Self::Arc(_) => "Arc",
            Self::Triangle(_) => "Triangle",
        }
    }
}

/// Every paint call one encode made, in order.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RecordedPaint {
    pub(crate) calls: Vec<PaintCall>,
}

impl RecordedPaint {
    /// Push the recorded sequence into `sink`. Calls re-enter through
    /// the provided half, so `sink` re-applies its own no-op gates —
    /// harmlessly, since a recorded call already passed them once.
    pub(crate) fn replay(&self, sink: &mut impl PaintSink) {
        for call in &self.calls {
            match call {
                PaintCall::Clip(p) => sink.clip(*p),
                PaintCall::PopClip => sink.pop_clip(),
                PaintCall::PushTransform(t) => sink.push_transform(*t),
                PaintCall::PopTransform => sink.pop_transform(),
                PaintCall::Rect(p) => sink.rect(*p),
                PaintCall::Shadow(p) => sink.shadow(*p),
                PaintCall::Text(p) => sink.text(*p),
                PaintCall::Mesh(p) => sink.mesh(*p),
                PaintCall::Polyline(p) => sink.polyline(*p),
                PaintCall::Image { payload, paint } => sink.image(*payload, paint.as_ref()),
                PaintCall::Curve(p) => sink.curve(*p),
                PaintCall::Arc(p) => sink.arc(*p),
                PaintCall::Triangle(p) => sink.triangle(*p),
            }
        }
    }

    /// Number of recorded calls matching `pred` — the shape most
    /// encoder assertions want ("how many clips did this subtree emit").
    pub(crate) fn count(&self, pred: impl Fn(&PaintCall) -> bool) -> usize {
        self.calls.iter().filter(|call| pred(call)).count()
    }
}

impl PaintSink for RecordedPaint {
    fn clip(&mut self, payload: PushClipPayload) {
        self.calls.push(PaintCall::Clip(payload));
    }

    fn pop_clip(&mut self) {
        self.calls.push(PaintCall::PopClip);
    }

    fn push_transform(&mut self, transform: TranslateScale) {
        self.calls.push(PaintCall::PushTransform(transform));
    }

    fn pop_transform(&mut self) {
        self.calls.push(PaintCall::PopTransform);
    }

    fn rect(&mut self, payload: DrawRectPayload) {
        self.calls.push(PaintCall::Rect(payload));
    }

    fn shadow(&mut self, payload: DrawShadowPayload) {
        self.calls.push(PaintCall::Shadow(payload));
    }

    fn text(&mut self, payload: DrawTextPayload) {
        self.calls.push(PaintCall::Text(payload));
    }

    fn mesh(&mut self, payload: DrawMeshPayload) {
        self.calls.push(PaintCall::Mesh(payload));
    }

    fn polyline(&mut self, payload: DrawPolylinePayload) {
        self.calls.push(PaintCall::Polyline(payload));
    }

    fn image(&mut self, payload: DrawImagePayload, paint: Option<&GpuPaintRef>) {
        self.calls.push(PaintCall::Image {
            payload,
            paint: paint.cloned(),
        });
    }

    fn curve(&mut self, payload: DrawCurvePayload) {
        self.calls.push(PaintCall::Curve(payload));
    }

    fn arc(&mut self, payload: DrawArcPayload) {
        self.calls.push(PaintCall::Arc(payload));
    }

    fn triangle(&mut self, payload: DrawTrianglePayload) {
        self.calls.push(PaintCall::Triangle(payload));
    }
}

/// Assert two encodes painted the same sequence, reporting the first
/// divergence by index and kind instead of dumping both call lists.
pub(crate) fn assert_same_paint(left: &RecordedPaint, right: &RecordedPaint) {
    for (i, (l, r)) in left.calls.iter().zip(&right.calls).enumerate() {
        assert!(
            l == r,
            "paint call {i} differs: {} vs {}\n  left:  {l:?}\n  right: {r:?}",
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
