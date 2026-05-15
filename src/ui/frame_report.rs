//! One frame's plain-data report from [`Ui::frame`]: the post-record
//! signals the host needs to act on. All frame-shaped state (forest,
//! layout, cascades, display) stays on [`Ui`] itself — `Frontend::build`
//! reads it directly via a `&Ui` borrow; the per-frame paint plan
//! ([`RenderPlan`], wrapped in `Option` for the skip case) is the only
//! render-shaped state this report carries.
//!
//! [`Ui`]: crate::ui::Ui
//! [`Ui::frame`]: crate::ui::Ui::frame

use crate::primitives::color::Color;
use crate::ui::damage::Damage;
use crate::ui::damage::region::DamageRegion;
use std::time::Duration;

/// Host-facing render plan, present only when there's actual render
/// work this frame — `FrameReport.plan = None` is the skip signal,
/// so neither the encoder nor the backend ever sees a no-op plan.
/// Pairs the engine's damage outcome with the surface clear colour
/// (needed by both variants: Full clears the colour attachment,
/// Partial pre-fills each scissor with the same colour before
/// painting).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RenderPlan {
    /// Clear + repaint the whole surface.
    Full { clear: Color },
    /// Load the backbuffer, then paint inside `region` after a
    /// `clear`-coloured pre-fill quad per scissor.
    Partial { clear: Color, region: DamageRegion },
}

impl RenderPlan {
    /// Build a render plan from `DamageEngine`'s output plus the
    /// surface clear colour. `Damage::None` ⇒ `None` (skip frame);
    /// `Full` / `Partial` ⇒ `Some(plan)`.
    pub(crate) fn from_damage(damage: Damage, clear: Color) -> Option<Self> {
        match damage {
            Damage::None => None,
            Damage::Full => Some(RenderPlan::Full { clear }),
            Damage::Partial(region) => Some(RenderPlan::Partial { clear, region }),
        }
    }
}

pub struct FrameReport {
    pub(crate) repaint_requested: bool,
    /// Absolute Ui-time deadline at which the host should wake and run
    /// another frame, even if no input arrives. `None` ⇒ no scheduled
    /// wake. Set by [`crate::ui::Ui::request_repaint_after`]. Hosts
    /// pair with `start + deadline → Instant` for
    /// `winit::ControlFlow::WaitUntil`.
    pub(crate) repaint_after: Option<Duration>,
    /// Per-frame render decision. `None` ⇒ skip path (backbuffer is
    /// correct); `Some(plan)` ⇒ work for the renderer.
    pub(crate) plan: Option<RenderPlan>,
}

impl FrameReport {
    /// `true` when an animation tick during this frame hasn't
    /// settled (set by `Ui::animate`). Hosts honor by calling
    /// `window.request_redraw()` (or equivalent) after present, so
    /// the next frame runs even when input is idle.
    pub fn repaint_requested(&self) -> bool {
        self.repaint_requested
    }

    /// Absolute Ui-time deadline for a deferred repaint. Compose with
    /// the host's clock anchor (e.g. `Host::start_instant() +
    /// deadline`) to get a wallclock `Instant`.
    pub fn repaint_after(&self) -> Option<Duration> {
        self.repaint_after
    }

    /// `true` when the renderer has nothing to do this frame — the
    /// previous backbuffer is correct. Hosts use this to skip the
    /// surface acquire / present cycle entirely.
    pub fn skip_render(&self) -> bool {
        self.plan.is_none()
    }
}
