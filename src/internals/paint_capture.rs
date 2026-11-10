//! Capturing [`PaintSink`] for tests and benches: [`PaintCapture`] holds the
//! call sequence as owned values, and [`PaintCapture::replay`] feeds it to
//! another sink so the compose bench can time compose alone. "Capture", not
//! "record", which the crate spends on authoring. It sits below the `draw`
//! no-op gate, so equal captures mean equal painted operations.

use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::renderer::frontend::paint_sink::PaintSink;
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

/// Declares [`PaintCall`] with its three lockstep transcriptions (variant
/// name, replay dispatch, [`PaintSink`] impl) from one table.
macro_rules! paint_calls {
    (
        $( $variant:ident($payload:ty) => $method:ident, )*
        --
        $( $unit:ident => $unit_method:ident, )*
    ) => {
        /// One recorded [`PaintSink`] call.
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub(crate) enum PaintCall {
            $( $variant($payload), )*
            $( $unit, )*
        }

        impl PaintCall {
            #[cfg(test)]
            pub(crate) const fn kind(&self) -> &'static str {
                match self {
                    $( Self::$variant(_) => stringify!($variant), )*
                    $( Self::$unit => stringify!($unit), )*
                }
            }

            /// Pushes this call into `sink` through the required half; see [`PaintCapture::replay`].
            fn replay_into(&self, sink: &mut impl PaintSink) {
                match self {
                    $( Self::$variant(payload) => sink.$method(*payload), )*
                    $( Self::$unit => sink.$unit_method(), )*
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
        }
    };
}

paint_calls! {
    PushClip(PushClipPayload) => push_clip,
    PushTransform(TranslateScale) => push_transform,
    Fill(DrawClosedPayload<ClosedFill>) => fill,
    Shadow(DrawShadowPayload) => shadow,
    Text(DrawTextPayload) => text,
    Mesh(DrawMeshPayload) => mesh,
    Stroke(DrawStrokePayload) => stroke,
    Icon(DrawIconPayload) => icon,
    Image(DrawClosedPayload<ClosedImage>) => image,
    Effect(DrawClosedPayload<ClosedEffect>) => effect,
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
    use crate::renderer::frontend::payload::draw_closed_payload::DrawClosedPayload;
    use crate::renderer::frontend::payload::draw_closed_payload::closed_image::ClosedImage;
    use crate::renderer::frontend::payload::draw_stroke_payload::DrawStrokePayload;
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

        pub(crate) fn as_stroke(&self) -> Option<&DrawStrokePayload> {
            match self {
                Self::Stroke(payload) => Some(payload),
                _ => None,
            }
        }

        pub(crate) fn as_image(&self) -> Option<&DrawClosedPayload<ClosedImage>> {
            match self {
                Self::Image(payload) => Some(payload),
                _ => None,
            }
        }
    }

    impl PaintCapture {
        /// The calls' kinds in order, so order assertions can tell a parent and child from siblings.
        pub(crate) fn kinds(&self) -> Vec<&'static str> {
            self.calls.iter().map(PaintCall::kind).collect()
        }

        /// Asserts `right` painted the same sequence, reporting the first divergence.
        pub(crate) fn assert_same(&self, right: &Self) {
            let left = self;
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
}
