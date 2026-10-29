//! The pane ledger: what each of a window's panes holds, and which take a new result this frame,
//! decided before the frame encodes.
//!
//! # Decisions
//!
//! - **The ledger decides which panes capture, before the frame encodes.** A pane captures on a
//!   whole frame, when damage left it stale, or when its held capture does not serve: another
//!   plan, or a footprint outside the held region. Each pane's [`PaneTicket`] says so, and every
//!   later stage reads it: the encoder keeps the draws under a capturing pane's footprint and no
//!   other's, the composer breaks the main pass only for a capturing pane, and a partial frame
//!   repaints each capturing footprint, as the target there still holds the last frame's pane.
//!   Past eight captures the frame paints whole, known before it builds. Refused: deciding after
//!   compose, which made every pane whose rect met the damage keep the draws under its footprint
//!   and break the pass, and built a frame twice past the cap.
//! - **The cascade's footprint is the one footprint.** The ledger, the encoder filter, the
//!   composer's break and the capture read [`PaneRow::footprint`](crate::cascade::pane_row::PaneRow::footprint),
//!   so the region a capture covers is the region the next frame's "serves" test reads. It holds
//!   the pane at every turn of a paint animation, so a turning pane keeps its capture. Refused: a
//!   composer footprint from the quad's turned, snapped rect, a pixel smaller, which the ledger
//!   cannot know before the frame encodes.
//! - **A pane the ledger holds nothing for captures.** That covers a new pane and one whose key
//!   moved, as a row removed before it gives it a new key; the damage walk marks such a pane
//!   stale too, so its rect repaints and the panes above it see it.
//! - **A pane that should take a new result and does not drops what it held.** A capturing pane
//!   that a fade, the occlusion pruner or the clear fold leaves undrawn keeps nothing, as damage
//!   under it went to no capture; it captures once it draws. A hidden pane that should not capture
//!   keeps its result. Refused: never pruning a pane, which pays a capture and a blur for a pane
//!   nobody sees.
//! - **A capturing frame that would copy more than the surface paints through the backbuffer.**
//!   The blurs then read the backbuffer in place, and the copy onto the target moves the surface
//!   once, where the captures move each footprint: on the m2 `cards/scrolling` took 4.8 % off,
//!   `cards_apart` 6.1 %. Every capturing frame through the backbuffer cost the 680M 12 % on
//!   `pair_near/scrolling`, two small footprints. A target that gives no copy, a GLES swapchain
//!   image, paints every capturing frame through it.
//! - **A pane with nothing painted under it shows the clear.** The composer gives a capturing pane
//!   whose footprint no draw before it touches barrier 0, so its backdrop is the clear, which a
//!   blur leaves unchanged: it neither breaks the pass nor blurs, and its footprint joins no
//!   scissor. A partial frame keeps every draw under a capturing pane's footprint, so a barrier
//!   of 0 there means nothing lies under it.

use crate::cascade::pane_row::PaneRow;
use crate::damage::Damage;
use crate::damage::dirty_panes::DirtyPanes;
use crate::display::Display;
use crate::primitives::geometry::urect::URect;
use crate::primitives::identity::pane_key::PaneKey;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_plan::RenderPlan;
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

/// What a frame does with one pane, decided before it encodes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PaneTicket {
    pub(crate) key: PaneKey,
    /// The physical px the pane's capture reads.
    pub(crate) footprint: URect,
    pub(crate) plan: BlurPlan,
    /// The texels of the plan's last level the pane's result covers: its held capture's, or this
    /// frame's where it captures.
    pub(crate) region: URect,
    /// Whether the pane takes a new result this frame, from a capture or the clear.
    pub(crate) captures: bool,
}

/// What a frame's damage left of its panes' captures.
#[derive(Clone, Copy, Debug)]
pub(crate) enum PaneDamage<'a> {
    /// A whole frame: every pane captures.
    Whole,
    /// A partial frame: the panes `dirty` holds are stale.
    Stale(&'a DirtyPanes),
}

impl<'a> PaneDamage<'a> {
    /// The panes a frame painted by `plan` left stale, `dirty` where it is partial.
    pub(crate) const fn of(plan: RenderPlan, dirty: &'a DirtyPanes) -> Self {
        match plan.damage {
            Damage::Full => Self::Whole,
            Damage::Partial(_) => Self::Stale(dirty),
        }
    }

    /// Whether a pane keyed `key` may keep what it holds.
    fn keeps(self, key: PaneKey) -> bool {
        match self {
            Self::Whole => false,
            Self::Stale(dirty) => !dirty.contains(key),
        }
    }
}

/// Whether a frame's captures fit a partial repaint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Settled {
    /// Every capturing pane's footprint joins the repaint scissors.
    Fits,
    /// More panes capture than a partial repaint holds scissors for: the frame paints whole, as
    /// the tickets already say.
    Overflow,
}

/// One window's pane ledger: the captures its panes hold, and this frame's [`PaneTicket`]s. The
/// backend keeps the results and follows each row's
/// [`source`](crate::renderer::render_buffer::backdrop_row::BackdropRow::source).
#[derive(Debug, Default)]
pub(crate) struct PaneCaptures {
    held: FxHashMap<PaneKey, Held>,
    /// The panes [`Self::decide`] took last, and a ticket for each; `None` for a pane whose
    /// footprint misses the surface, which paints nothing.
    rows: Vec<PaneRow>,
    tickets: Vec<Option<PaneTicket>>,
}

impl PaneCaptures {
    /// The most panes a partial frame captures; past it the frame paints whole.
    pub(crate) const PARTIAL_CAP: usize = DirtyPanes::CAP;

    /// Write a ticket for each of `panes`, a frame's in paint order on `display`. Every pane
    /// captures on a whole frame, else each one `damage` left stale or whose held capture does
    /// not serve. When more capture than a partial repaint holds, the tickets are a whole
    /// frame's, and [`Settled::Overflow`] says the frame paints whole. Holds nothing new:
    /// [`Self::settle`] records what the built frame took.
    pub(crate) fn decide<'a>(
        &mut self,
        panes: impl IntoIterator<Item = &'a PaneRow>,
        display: Display,
        damage: PaneDamage<'_>,
    ) -> Settled {
        self.rows.clear();
        self.rows.extend(panes.into_iter().copied());
        self.decide_rows(display, damage)
    }

    fn decide_rows(&mut self, display: Display, damage: PaneDamage<'_>) -> Settled {
        let surface = display.physical;
        self.tickets.clear();
        let mut captures = 0;
        for pane in &self.rows {
            let footprint = pane.physical_footprint(display);
            let ticket = pane.plan.region(footprint, surface).map(|region| {
                let held = damage
                    .keeps(pane.key)
                    .then(|| self.held.get(&pane.key))
                    .flatten()
                    .filter(|held| held.serves(pane.plan, region, surface));
                captures += usize::from(held.is_none());
                PaneTicket {
                    key: pane.key,
                    footprint,
                    plan: pane.plan,
                    region: held.map_or(region, |held| held.region),
                    captures: held.is_none(),
                }
            });
            self.tickets.push(ticket);
        }
        if matches!(damage, PaneDamage::Whole) || captures <= Self::PARTIAL_CAP {
            Settled::Fits
        } else {
            self.decide_rows(display, PaneDamage::Whole);
            Settled::Overflow
        }
    }

    /// The tickets [`Self::decide`] wrote last.
    pub(crate) fn tickets(&self) -> &[Option<PaneTicket>] {
        &self.tickets
    }

    /// Record what the frame `buffer` holds, built from the last tickets, took: each pane that
    /// took a new result holds it, each capturing pane that took none holds nothing, and the
    /// panes [`Self::decide`] took no row for, no longer on screen, drop theirs.
    pub(crate) fn settle(&mut self, buffer: &RenderBuffer) {
        let rows = &self.rows;
        self.held
            .retain(|key, _| rows.iter().any(|pane| pane.key == *key));
        for ticket in self
            .tickets
            .iter()
            .flatten()
            .filter(|ticket| ticket.captures)
        {
            self.held.remove(&ticket.key);
        }
        let surface = buffer.display.physical;
        for row in &buffer.backdrops {
            if row.source.is_new() {
                self.held.insert(
                    row.key,
                    Held {
                        plan: row.plan,
                        surface,
                        region: row.region,
                    },
                );
            }
        }
    }
}

#[cfg(test)]
mod tests;
