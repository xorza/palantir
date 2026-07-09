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

/// WindowRenderer-facing render plan, present only when there's actual render
/// work this frame — `FrameReport.plan = None` is the skip signal, so neither
/// the encoder nor the backend ever sees a no-op plan. Pairs the surface clear
/// colour (needed for both kinds: `Full` clears the colour attachment,
/// `Partial` pre-fills each scissor with it) with the [`RenderKind`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderPlan {
    /// Surface clear colour for this frame.
    pub clear: Color,
    /// Whole surface, or just a damage region.
    pub kind: RenderKind,
}

/// What a [`RenderPlan`] repaints.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RenderKind {
    /// Clear + repaint the whole surface.
    Full,
    /// Load the backbuffer, then paint inside `region` after a
    /// `clear`-coloured pre-fill quad per scissor.
    Partial { region: DamageRegion },
}

impl RenderPlan {
    /// Build a render plan from `DamageEngine`'s output plus the
    /// surface clear colour. `Damage::Skip` ⇒ `None` (skip frame);
    /// `Full` / `Partial` ⇒ `Some(plan)`.
    pub(crate) fn from_damage(damage: Damage, clear: Color) -> Option<Self> {
        let kind = match damage {
            Damage::Skip => return None,
            Damage::Full => RenderKind::Full,
            Damage::Partial(region) => RenderKind::Partial { region },
        };
        Some(RenderPlan { clear, kind })
    }

    /// This plan escalated to a full repaint, keeping its clear colour — used
    /// when partial damage can't be honoured (direct present, or a freshly
    /// (re)created backbuffer with undefined contents).
    pub(crate) fn to_full(self) -> RenderPlan {
        RenderPlan {
            clear: self.clear,
            kind: RenderKind::Full,
        }
    }
}

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

#[derive(Debug)]
pub struct FrameReport {
    /// `true` when an animation tick during this frame hasn't
    /// settled (set by `Ui::animate`). Hosts honor by calling
    /// `window.request_redraw()` (or equivalent) after present, so
    /// the next frame runs even when input is idle.
    pub repaint_requested: bool,
    /// Absolute Ui-time deadline at which the host should wake and run
    /// another frame, even if no input arrives. `None` ⇒ no scheduled
    /// wake. Set by [`crate::ui::Ui::request_repaint_after`]. Hosts
    /// pair with `start + deadline → Instant` (e.g.
    /// `WindowRenderer::start_instant() + deadline`) for
    /// `winit::ControlFlow::WaitUntil`.
    pub repaint_after: Option<Duration>,
    /// Per-frame render decision. `None` ⇒ skip path (the previous
    /// backbuffer is correct — hosts skip the surface acquire /
    /// present cycle entirely); `Some(plan)` ⇒ work for the renderer.
    pub plan: Option<RenderPlan>,
    /// How `Ui::frame` resolved this frame — informational, used by
    /// tests / benches / profilers to assert the short-circuit fired
    /// or the double-layout retry didn't. See [`FrameProcessing`].
    pub processing: FrameProcessing,
}

#[cfg(any(test, feature = "internals"))]
pub mod test_support {
    #![allow(dead_code)]
    use crate::primitives::rect::Rect;
    use crate::ui::frame_report::*;

    impl FrameReport {
        /// Overwrite `self.plan`: empty ⇒ `Full { clear }`, otherwise `Partial`
        /// built by adding each rect.
        pub fn force_damage_to_rects(&mut self, rects: &[Rect], clear: Color) {
            if rects.is_empty() {
                self.plan = Some(RenderPlan {
                    clear,
                    kind: RenderKind::Full,
                });
                return;
            }
            self.plan = Some(RenderPlan {
                clear,
                kind: RenderKind::Partial {
                    region: DamageRegion::from_rects(rects),
                },
            });
        }
    }
}
