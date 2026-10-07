//! Capturing [`PaintSink`] for tests and benches: [`PaintCapture`] holds the
//! call sequence as owned values, and [`PaintCapture::replay`] feeds it to
//! another sink so the compose bench can time compose alone. "Capture", not
//! "record", which the crate spends on authoring. It sits below the `draw_*`
//! no-op gates, so equal captures mean equal painted operations.

use crate::primitives::geometry::translate_scale::TranslateScale;
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

/// Declares [`PaintCall`] with its three lockstep transcriptions (variant
/// name, replay dispatch, [`PaintSink`] impl) from one table. `image` is
/// written by hand: its sink method takes a second argument.
macro_rules! paint_calls {
    (
        $( $variant:ident($payload:ty) => $method:ident, )*
        --
        $( $unit:ident => $unit_method:ident, )*
    ) => {
        /// One recorded [`PaintSink`] call; a `GpuView` composite records as [`Self::Image`] with its callback.
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
            #[cfg(test)]
            pub(crate) const fn kind(&self) -> &'static str {
                match self {
                    $( Self::$variant(_) => stringify!($variant), )*
                    $( Self::$unit => stringify!($unit), )*
                    Self::Image { .. } => "Image",
                }
            }

            /// Pushes this call into `sink` through the required half; see [`PaintCapture::replay`].
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

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PaintCapture {
    pub(crate) calls: Vec<PaintCall>,
}

impl PaintCapture {
    /// Pushes the sequence into `sink` through the required half, below the
    /// no-op gate, so replay reproduces the stream exactly. Only sound for
    /// calls that came from a recording.
    pub(crate) fn replay(&self, sink: &mut impl PaintSink) {
        for call in &self.calls {
            call.replay_into(sink);
        }
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::internals::paint_capture::{PaintCall, PaintCapture};
    use crate::renderer::frontend::payload::draw_curve_payload::DrawCurvePayload;
    use crate::renderer::frontend::payload::draw_image_payload::DrawImagePayload;
    use crate::renderer::frontend::payload::draw_text_payload::DrawTextPayload;
    use crate::renderer::frontend::payload::push_clip_payload::PushClipPayload;

    /// Typed reads of one call's payload.
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
        /// The calls' kinds in order, so order assertions can tell a parent and child from siblings.
        pub(crate) fn kinds(&self) -> Vec<&'static str> {
            self.calls.iter().map(PaintCall::kind).collect()
        }
    }

    /// Asserts two encodes painted the same sequence, reporting the first divergence.
    pub(crate) fn assert_same_capture(left: &PaintCapture, right: &PaintCapture) {
        for (i, (l, r)) in left.calls.iter().zip(&right.calls).enumerate() {
            // Compares rendered `Debug`, not `PartialEq`: `NaN != NaN` would fail
            // identical frames and `-0.0 == 0.0` would hide a sign-of-zero drift.
            let (ls, rs) = (format!("{l:?}"), format!("{r:?}"));
            assert!(
                ls == rs,
                "paint call {i} differs: {} vs {}\n  left:  {ls}\n  right: {rs}",
                l.kind(),
                r.kind(),
            );
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
