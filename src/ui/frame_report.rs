//! One frame's plain-data report from [`Ui::frame`]: the post-record
//! signals a caller may inspect. All frame-shaped state (forest,
//! layout, cascade, display) stays on [`Ui`] itself. The renderer's
//! detailed paint plan remains crate-private; callers see its stable
//! [`FramePaint`] classification.
//!
//! [`Ui`]: crate::ui::Ui
//! [`Ui::frame`]: crate::ui::Ui::frame

use crate::renderer::render_plan::RenderPlan;
use crate::scene::damage::Damage;
use std::time::Duration;

/// How `Ui::frame` resolved this frame: which passes actually ran.
///
/// Crate-private, for the same reason the [`RenderPlan`] behind
/// [`FrameReport::paint`] is: it names the internal pass structure, and
/// that structure is free to change. A consumer asking "was anything
/// repainted" reads [`FramePaint`]; this answers "which passes got
/// there", which only the crate's own tests have a stake in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrameProcessing {
    /// Paint-anim-only short-circuit fired: no pre_record, no user
    /// closure, no post_record, no layout, no cascade. Just damage
    /// compute + encode + paint against the retained tree.
    PaintOnly,
    /// Standard frame: one record pass + layout + cascade + damage
    /// + finalize.
    SingleLayout,
    /// Pass A's closure set the action flag or requested relayout,
    /// so a second `record_pass` (plus its own `post_record` +
    /// layout + cascade) ran before `finalize_frame`. Capped at
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
    /// Which passes ran. Gated with its readers — the crate's own
    /// tests, asserting that the paint-only short-circuit fired or that
    /// the double-layout retry didn't. The value itself is live in every
    /// build; `FrameRuntime::note_processing` is what consumes it.
    /// See [`FrameProcessing`].
    #[cfg(test)]
    pub(crate) processing: FrameProcessing,
}

impl FrameReport {
    /// Classify this frame without exposing renderer-only damage data.
    pub const fn paint(&self) -> FramePaint {
        match self.plan {
            None => FramePaint::Skip,
            Some(RenderPlan {
                damage: Damage::Full,
                ..
            }) => FramePaint::Full,
            Some(RenderPlan {
                damage: Damage::Partial(..),
                ..
            }) => FramePaint::Partial,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::color::RgbaF32;
    use crate::primitives::rect::Rect;
    use crate::renderer::render_plan::RenderPlan;
    use crate::scene::damage::Damage;
    use crate::scene::damage::region::DamageRegion;
    use crate::ui::frame_report::{FramePaint, FrameProcessing, FrameReport};

    #[test]
    fn paint_classifies_every_render_plan_shape() {
        let cases = [
            (None, FramePaint::Skip),
            (
                Some(RenderPlan {
                    clear: RgbaF32::BLACK,
                    damage: Damage::Full,
                }),
                FramePaint::Full,
            ),
            (
                Some(RenderPlan {
                    clear: RgbaF32::BLACK,
                    damage: Damage::Partial(
                        DamageRegion::from(Rect::new(1.0, 2.0, 3.0, 4.0)).unmeasured(),
                    ),
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
