//! One frame's plain-data report from [`Ui::frame`]: the post-record
//!
//! [`Ui`]: crate::ui::Ui
//! [`Ui::frame`]: crate::ui::Ui::frame

use crate::damage::Damage;
use crate::primitives::geometry::rect::Rect;
use crate::renderer::render_plan::RenderPlan;
use std::time::Duration;

/// How `Ui::frame` resolved this frame: which passes ran. Crate-private, since it names the internal pass structure; consumers read [`FramePaint`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrameProcessing {
    /// Paint-anim-only short-circuit: no record, layout or cascade; only damage + encode + paint against the retained tree.
    PaintOnly,
    /// Standard frame: one record pass + layout + cascade + damage + finalize.
    SingleLayout,
    /// Pass A set the action flag or requested relayout, so a second `record_pass` ran (one retry per frame).
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

/// What one call to [`Ui::frame`](crate::Ui) did, and what the host owes
/// the next one.
#[derive(Debug)]
pub struct FrameReport {
    /// `true` when an animation tick this frame hasn't settled (set by `Ui::animate`); hosts request a redraw after present.
    pub repaint_requested: bool,
    /// Absolute Ui-time deadline at which the host should wake and run another frame; `None` for no scheduled wake. Set by [`crate::Ui::request_repaint_after`].
    pub repaint_after: Option<Duration>,
    pub(crate) plan: Option<RenderPlan>,
    /// Which passes ran, see [`FrameProcessing`]; `FrameCycle::run` asserts it against the paint outcome and tests read it.
    pub(crate) processing: FrameProcessing,
    /// Where IME text goes this frame: the caret rect (logical px) a widget asked for with [`Ui::request_ime`](crate::Ui::request_ime), else `None`. A host that embeds the UI enables its input method while `Some`; the winit host does it itself.
    pub ime_area: Option<Rect>,
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
                repaint_after: None,
                plan,
                processing: FrameProcessing::SingleLayout,
                ime_area: None,
            };
            assert_eq!(report.paint(), expected);
        }
    }
}
