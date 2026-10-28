//! Which of a frame's panes capture their backdrop again, decided before the frame paints.
//!
//! # Decisions
//!
//! - **The CPU decides which panes capture.** A pane captures on a whole frame, when damage left
//!   it stale, or when its held capture does not serve: another plan, or a footprint outside the
//!   held region. Deciding before the frame paints lets a partial frame repaint each capturing
//!   footprint, which the repaint scissors and the cutout census both read, as the target there
//!   still holds the last frame's pane; past eight captures the frame paints whole. A target
//!   that gives no copy, a GLES swapchain image, paints a capturing frame through the
//!   backbuffer. Refused: the backend deciding after the CPU built the scissors.

use crate::damage::dirty_panes::DirtyPanes;
use crate::primitives::geometry::urect::URect;
use crate::primitives::identity::pane_key::PaneKey;
use crate::renderer::render_buffer::RenderBuffer;
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
/// [`capture`](crate::renderer::render_buffer::backdrop_row::BackdropRow::capture).
#[derive(Debug, Default)]
pub(crate) struct PaneCaptures {
    held: FxHashMap<PaneKey, Held>,
}

impl PaneCaptures {
    /// The most panes a partial frame captures; past it the frame paints whole.
    pub(crate) const PARTIAL_CAP: usize = DirtyPanes::CAP;

    /// Mark each of `buffer`'s panes that captures this frame: all of them on a `full` frame,
    /// else those `dirty` holds and those whose held capture does not serve. Drops the captures
    /// of panes no longer on screen.
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
            row.capture = !kept;
            if row.capture {
                captures += 1;
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
