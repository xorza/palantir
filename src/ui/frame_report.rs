//! One frame's plain-data report from [`Ui::frame`]: the post-record
//! signals a caller may inspect. All frame-shaped state (forest,
//! layout, cascades, display) stays on [`Ui`] itself. The renderer's
//! detailed paint plan remains crate-private; callers see its stable
//! [`FramePaint`] classification.
//!
//! [`Ui`]: crate::ui::Ui
//! [`Ui::frame`]: crate::ui::Ui::frame

use crate::renderer::plan::{RenderKind, RenderPlan};
use std::time::Duration;

/// How `Ui::frame` resolved this frame — informational, useful to
/// tests / benches / profilers asking "did the short-circuit fire?"
/// or "did the relayout retry kick in?". Self-classifying so callers
/// don't need to derive it from the renderer plan or input flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameProcessing {
    /// Paint-anim-only short-circuit fired: no pre_record, no user
    /// closure, no post_record, no layout, no cascades. Just damage
    /// compute + encode + paint against the retained tree.
    PaintOnly,
    /// Standard frame: one record pass + layout + cascades + damage
    /// + finalize.
    SingleLayout,
    /// Pass A's closure set the action flag or requested relayout,
    /// so a second `record_pass` (plus its own `post_record` +
    /// layout + cascades) ran before `finalize_frame`. Capped at
    /// one retry per `Ui::frame`.
    DoubleLayout,
}

/// How much of the output this frame repaints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FramePaint {
    /// The previous output remains current; no paint work ran.
    Skip,
    /// The whole output repaints.
    Full,
    /// Only the internally tracked damage region repaints.
    Partial,
}

#[derive(Debug)]
pub struct FrameReport {
    /// `true` when an animation tick during this frame hasn't
    /// settled (set by `Ui::animate`). Hosts honor by calling
    /// `window.request_redraw()` (or equivalent) after present, so
    /// the next frame runs even when input is idle.
    pub repaint_requested: bool,
    /// Absolute Ui-time deadline at which the host should wake and run
    /// another frame, even if no input arrives. `None` ⇒ no scheduled
    /// wake. Set by [`crate::Ui::request_repaint_after`]. The supported host
    /// facades convert this Ui-time deadline to their own clock.
    pub repaint_after: Option<Duration>,
    pub(crate) plan: Option<RenderPlan>,
    /// How `Ui::frame` resolved this frame — informational, used by
    /// tests / benches / profilers to assert the short-circuit fired
    /// or the double-layout retry didn't. See [`FrameProcessing`].
    pub processing: FrameProcessing,
}

impl FrameReport {
    /// Classify this frame without exposing renderer-only damage data.
    pub const fn paint(&self) -> FramePaint {
        match self.plan {
            None => FramePaint::Skip,
            Some(RenderPlan {
                kind: RenderKind::Full,
                ..
            }) => FramePaint::Full,
            Some(RenderPlan {
                kind: RenderKind::Partial { .. },
                ..
            }) => FramePaint::Partial,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::color::Color;
    use crate::primitives::rect::Rect;
    use crate::renderer::plan::{RenderKind, RenderPlan};
    use crate::scene::damage::region::DamageRegion;
    use crate::ui::frame_report::{FramePaint, FrameProcessing, FrameReport};

    #[test]
    fn paint_classifies_every_render_plan_shape() {
        let cases = [
            (None, FramePaint::Skip),
            (
                Some(RenderPlan {
                    clear: Color::BLACK,
                    kind: RenderKind::Full,
                }),
                FramePaint::Full,
            ),
            (
                Some(RenderPlan {
                    clear: Color::BLACK,
                    kind: RenderKind::Partial {
                        region: DamageRegion::from(Rect::new(1.0, 2.0, 3.0, 4.0)),
                    },
                }),
                FramePaint::Partial,
            ),
        ];

        for (plan, expected) in cases {
            let report = FrameReport {
                repaint_requested: false,
                repaint_after: None,
                plan,
                processing: FrameProcessing::SingleLayout,
            };
            assert_eq!(report.paint(), expected);
        }
    }
}
