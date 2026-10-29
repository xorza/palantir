//! Which of a frame's panes capture their backdrop again, decided before the frame paints.
//!
//! # Decisions
//!
//! - **The CPU decides which panes capture.** A pane captures on a whole frame, when damage left
//!   it stale, or when its held capture does not serve: another plan, or a footprint outside the
//!   held region. Deciding before the frame paints lets a partial frame repaint each capturing
//!   footprint, which the repaint scissors and the cutout census both read, as the target there
//!   still holds the last frame's pane; past eight captures the frame paints whole. Refused: the
//!   backend deciding after the CPU built the scissors.
//! - **A capturing frame that would copy more than the surface paints through the backbuffer.**
//!   The blurs then read the backbuffer in place, and the copy onto the target moves the surface
//!   once, where the captures move each footprint: on the m2 `cards/scrolling` took 4.8 % off,
//!   `cards_apart` 6.1 %. Every capturing frame through the backbuffer cost the 680M 12 % on
//!   `pair_near/scrolling`, two small footprints. A target that gives no copy, a GLES swapchain
//!   image, paints every capturing frame through it.
//! - **A pane with nothing painted under it shows the clear.** The composer gives a pane whose
//!   footprint no draw before it touches barrier 0, so its backdrop is the clear, which a blur
//!   leaves unchanged: it neither breaks the pass nor blurs, and its footprint joins no scissor.
//!   Only where it would capture: a partial frame keeps every draw under a capturing pane's
//!   footprint, so a barrier of 0 there means nothing lies under it, where a held pane's barrier
//!   counts only the draws the damage kept.
//! - **A hidden pane drops its capture.** A pane whose quad a later opaque quad covers, which
//!   the occlusion pruner or the clear fold drops, neither captures nor keeps what it held:
//!   damage under it while it is hidden reaches no capture, so a kept result would be stale when
//!   it shows. It captures anew then, as the cover's leaving damages its rect. Refused: never
//!   pruning a pane, which keeps its result fresh at the cost of a capture and a blur a frame for
//!   a pane nobody sees.

use crate::damage::dirty_panes::DirtyPanes;
use crate::primitives::geometry::urect::URect;
use crate::primitives::identity::pane_key::PaneKey;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::backdrop_row::PaneSource;
use crate::shape::paint::blur_plan::BlurPlan;
use glam::UVec2;
use rustc_hash::FxHashMap;

/// What a pane's held capture blurred: its plan, its surface and its region in texels of the
/// plan's last level.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Held {
    plan: BlurPlan,
    surface: UVec2,
    region: URect,
}

impl Held {
    /// Whether this capture serves a pane of `plan` at `region` on `surface`: blurred alike over a
    /// region that holds it. The pane's texels then depend only on texels inside the held one.
    fn serves(&self, plan: BlurPlan, region: URect, surface: UVec2) -> bool {
        self.plan == plan && self.surface == surface && self.region.union(region) == self.region
    }
}

/// Whether a frame's captures fit a partial repaint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Settled {
    /// Every capturing pane's footprint joins the repaint scissors.
    Fits,
    /// More panes capture than a partial repaint holds scissors for: the frame paints whole.
    Overflow,
}

/// One window's held captures, so the CPU knows before a frame paints which panes capture again
/// and repaints their footprints: the main pass reads a capture from the target, where the last
/// frame's pane still lies under it. The backend keeps the results and follows each row's
/// [`source`](crate::renderer::render_buffer::backdrop_row::BackdropRow::source).
#[derive(Debug, Default)]
pub(crate) struct PaneCaptures {
    held: FxHashMap<PaneKey, Held>,
}

impl PaneCaptures {
    /// The most panes a partial frame captures; past it the frame paints whole.
    pub(crate) const PARTIAL_CAP: usize = DirtyPanes::CAP;

    /// Mark where each of `buffer`'s panes takes its result this frame. On a `full` frame every
    /// pane takes a new one, else each one `dirty` holds or whose held capture does not serve:
    /// from the clear when nothing is painted before it, else from a capture. Drops the captures
    /// of panes no longer on screen, and of hidden panes, which stay hidden.
    pub(crate) fn settle(
        &mut self,
        buffer: &mut RenderBuffer,
        full: bool,
        dirty: &DirtyPanes,
    ) -> Settled {
        let live = &buffer.live_panes;
        self.held.retain(|key, _| live.contains(key));
        let surface = buffer.display.physical;
        let mut captures = 0;
        for row in &mut buffer.backdrops {
            if row.source == PaneSource::Hidden {
                self.held.remove(&row.key);
                continue;
            }
            let region = row
                .plan
                .region(row.footprint, surface)
                .expect("a pane's footprint lies on the surface");
            let kept = !full
                && !dirty.contains(row.key)
                && self
                    .held
                    .get(&row.key)
                    .is_some_and(|held| held.serves(row.plan, region, surface));
            row.source = if kept {
                PaneSource::Held
            } else if row.barrier == 0 {
                PaneSource::Clear
            } else {
                captures += 1;
                PaneSource::Capture
            };
            if !kept {
                self.held.insert(
                    row.key,
                    Held {
                        plan: row.plan,
                        surface,
                        region,
                    },
                );
            }
        }
        if full || captures <= Self::PARTIAL_CAP {
            Settled::Fits
        } else {
            Settled::Overflow
        }
    }
}

#[cfg(test)]
mod tests;
