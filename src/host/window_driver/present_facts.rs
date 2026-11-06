//! How a frame reaches its target, worked out from plain facts with no device.

use crate::damage::Damage;
use crate::host::window_driver::{PresentPath, PresentStrategy};
use crate::renderer::render_plan::RenderPlan;
use glam::UVec2;

/// The density at which [`promote_share`] reads one half, in rows that draw on the surface per
/// million physical pixels.
pub(crate) const PROMOTE_HALF_ROWS_PER_MPX: f32 = 400.0;

/// What all of the surface under pane footprints counts for in [`promote_share`]'s density, in the
/// same rows per million physical pixels.
pub(crate) const PROMOTE_PANE_ROWS_PER_MPX: f32 = 300.0;

/// A scene's rows per million physical pixels.
pub(crate) fn rows_per_mpx(row_count: usize, physical: UVec2) -> f32 {
    row_count as f32 / (physical.x as f32 * physical.y as f32 / 1e6)
}

/// The share of the surface a damage may cover before a whole repaint is cheaper, at a density
/// that reads one half at `half_rows_per_mpx`.
pub(crate) fn density_share(rows_per_mpx: f32, half_rows_per_mpx: f32) -> f32 {
    rows_per_mpx / (rows_per_mpx + half_rows_per_mpx)
}

/// The share of the surface a damage may cover before [`PresentStrategy::DirectAdaptive`] paints a
/// whole frame direct: `d / (d + 400)` of the density `d`, the rows that draw on the surface per
/// million physical pixels, and 300 for all of the surface under pane footprints.
///
/// A partial frame pays a copy of the whole surface, and saves the rows and the pixels outside its
/// damage and the captures of the panes it leaves, so a denser scene makes more damage worth a
/// partial frame, and an empty one makes none. Both sides scale with the surface, which leaves the
/// density. The numbers are in `.notes/PERF.md`.
///
/// Refused: one share, 0.4 when this was written, whose best value ran from 0.1 on a plain screen
/// to 0.7 on a dense one; the bounds as the measure, on a tiler too, whose partial frame moves them;
/// a lower share on a stale backbuffer, and a first whole frame through the backbuffer, which helped
/// one GPU and cost another.
pub(super) fn promote_share(row_count: usize, pane_share: f32, physical: UVec2) -> f32 {
    density_share(
        rows_per_mpx(row_count, physical) + PROMOTE_PANE_ROWS_PER_MPX * pane_share,
        PROMOTE_HALF_ROWS_PER_MPX,
    )
}

/// The whole path a refused partial frame takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Escalation {
    /// The path of a whole frame under the strategy.
    ByStrategy,
    /// Through the backbuffer: a target that cannot keep its pixels paints there, and a whole frame
    /// leaves the backbuffer ready for the partial frames after it.
    ThroughBackbuffer,
}

/// What [`Self::path`] reads beside the plan, fixed for the frame.
#[derive(Clone, Copy, Debug)]
pub(super) struct PresentFacts {
    pub(super) strategy: PresentStrategy,
    /// The frame takes a screenshot, which reads the backbuffer, so it paints all of it there
    /// whatever its damage or the strategy.
    pub(super) capturing: bool,
    pub(super) backbuffer_fresh: bool,
    /// [`promote_share`] of a partial frame. It walks the paint rows, so no other frame pays.
    pub(super) promote_share: Option<f32>,
}

impl PresentFacts {
    /// How a frame of `plan` reaches its target; `None` is a frame with nothing to draw.
    pub(super) fn path(self, plan: Option<RenderPlan>) -> PresentPath {
        let Some(plan) = plan else {
            return match self.strategy {
                PresentStrategy::BackbufferCopy => PresentPath::SkipCopy,
                PresentStrategy::DirectAdaptive | PresentStrategy::Retained => {
                    PresentPath::SkipNoop
                }
            };
        };
        if self.capturing {
            return PresentPath::ViaBackbuffer(plan.to_full());
        }
        match self.strategy {
            PresentStrategy::DirectAdaptive => match plan.damage {
                Damage::Full => PresentPath::Direct(plan),
                Damage::Partial(damage) => {
                    let share = self.promote_share.expect("a partial frame has its share");
                    if damage.coverage > share {
                        PresentPath::Direct(plan.to_full())
                    } else if self.backbuffer_fresh {
                        PresentPath::ViaBackbuffer(plan)
                    } else {
                        PresentPath::ViaBackbuffer(plan.to_full())
                    }
                }
            },
            PresentStrategy::BackbufferCopy => PresentPath::ViaBackbuffer(plan),
            PresentStrategy::Retained => PresentPath::Direct(plan),
        }
    }

    /// The path of `whole`, the plan a partial frame refused by `escalation` paints instead.
    pub(super) fn whole_path(self, whole: RenderPlan, escalation: Escalation) -> PresentPath {
        match escalation {
            Escalation::ByStrategy => self.path(Some(whole)),
            Escalation::ThroughBackbuffer => PresentPath::ViaBackbuffer(whole),
        }
    }
}
