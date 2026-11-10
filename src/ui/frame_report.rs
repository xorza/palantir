//! One frame's plain-data report from [`WindowFrames::frame`](crate::ui::window_frames::WindowFrames::frame).

use crate::damage::Damage;
use crate::renderer::render_plan::RenderPlan;
use std::time::Duration;

/// How `WindowFrames::frame` resolved this frame: which passes ran. Crate-private, since it names the internal pass structure; consumers read [`FramePaint`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrameProcessing {
    /// Paint-anim-only short-circuit: no record, layout or cascade; only damage + encode + paint against the retained tree.
    PaintOnly,
    /// Standard frame: one record pass + layout + cascade + damage + finalize.
    SingleLayout,
    /// Pass A set the action flag or requested relayout, so a second `record_pass` ran (one retry per frame).
    DoubleLayout,
}

impl FrameProcessing {
    /// Whether the frame ran the app's record: what may have changed the app
    /// every window records from. A paint-only frame ran none of it.
    #[cfg(feature = "winit")]
    pub(crate) const fn ran_app(self) -> bool {
        !matches!(self, Self::PaintOnly)
    }
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

/// What one frame did, and what the host owes
/// the next one.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameReport {
    /// Whether another frame is owed after this one: an animation still moving, a widget or the app that asked ([`Ui::request_repaint`](crate::Ui::request_repaint)), a screenshot that landed or input held for the next frame. A host requests a redraw after present.
    pub repaint_requested: bool,
    /// Absolute Ui-time deadline at which the host should wake and run another frame; `None` for no scheduled wake. Set by [`crate::Ui::request_repaint_after`] and by a paint animation's next step.
    pub repaint_at: Option<Duration>,
    /// Absolute Ui-time before which the host starts no next frame, whatever asks for it; `None`
    /// while the frame rate is full ([`Ui::is_frame_rate_reduced`](crate::Ui::is_frame_rate_reduced)). It falls on a
    /// grid of whole refresh periods at or above 1/30 s, so frames run at most at 30 Hz and land on
    /// a vsync. A host on a real clock may start the frame up to half a refresh period sooner, or
    /// 1/240 s where the rate is unknown, so it presents at the vsync of that time.
    pub earliest_repaint_at: Option<Duration>,
    pub(crate) plan: Option<RenderPlan>,
    /// Which passes ran, see [`FrameProcessing`]; `WindowFrames::frame` asserts it against the paint outcome and tests read it.
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
    use crate::damage::Damage;
    use crate::damage::region::DamageRegion;
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::paint::color::RgbaF32;
    use crate::renderer::render_plan::RenderPlan;
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
                repaint_at: None,
                earliest_repaint_at: None,
                plan,
                processing: FrameProcessing::SingleLayout,
            };
            assert_eq!(report.paint(), expected);
        }
    }
}
