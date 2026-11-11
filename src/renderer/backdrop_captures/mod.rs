//! The backdrop ledger: what each of a window's backdrops holds, and which take a new result this
//! frame, decided before the frame encodes.
//!
//! # Decisions
//!
//! - **The ledger decides which backdrops capture, before the frame encodes.** A backdrop captures
//!   on a whole frame, when damage left it stale, or when its held capture does not serve: another
//!   plan, or a footprint outside the held region. Each backdrop's [`BackdropTicket`] says so, and
//!   every later stage reads it: the encoder keeps the draws under a capturing backdrop's footprint
//!   and no other's, the composer breaks the main pass only for a capturing backdrop, and a partial
//!   frame repaints each capturing footprint, as the target there still holds the last frame's
//!   backdrop. Past eight captures the frame paints whole, known before it builds. Refused:
//!   deciding after compose, which made every backdrop whose rect met the damage keep the draws
//!   under its footprint and break the pass, and built a frame twice past the cap.
//! - **The cascade's footprint is the one footprint.** The ledger, the encoder filter, the
//!   composer's break and the capture read
//!   [`PlaneBackdrop::footprint`](crate::cascade::plane_backdrop::PlaneBackdrop::footprint), so the
//!   region a capture covers is the region the next frame's "serves" test reads. It holds the
//!   backdrop at every turn of a paint animation, so a turning backdrop keeps its capture. Refused:
//!   a composer footprint from the quad's turned, snapped rect, a pixel smaller, which the ledger
//!   cannot know before the frame encodes.
//! - **A backdrop the ledger holds nothing for captures.** That covers a new backdrop and one whose
//!   key moved, as a row removed before it gives it a new key; the damage walk marks such a
//!   backdrop stale too, so its rect repaints and the backdrops above it see it.
//! - **A backdrop that should take a new result and does not drops what it held.** A capturing
//!   backdrop that a fade or the clear fold leaves undrawn keeps nothing, as damage under it went
//!   to no capture; it captures once it draws. A hidden backdrop that should not capture keeps its
//!   result. Refused: keeping the backdrops the clear fold covers, which pays a capture and a blur
//!   for a backdrop nobody sees.
//! - **A capturing frame that would copy more than the surface paints through the backbuffer.**
//!   The blurs then read the backbuffer in place, and the copy onto the target moves the surface
//!   once, where the captures move each footprint: on an Apple laptop `cards/scrolling` took 4.8 % off,
//!   `cards_apart` 6.1 %. Every capturing frame through the backbuffer cost a mobile AMD iGPU 12 % on
//!   `pair_near/scrolling`, two small footprints. A target that gives no copy, a GLES swapchain
//!   image, paints every capturing frame through it.
//! - **A backdrop with nothing painted under it shows the clear.** The composer gives a capturing
//!   backdrop whose footprint no draw before it touches barrier 0, so its backdrop is the clear,
//!   which a blur leaves unchanged: it neither breaks the pass nor blurs, and its footprint joins
//!   no scissor. A partial frame keeps every draw under a capturing backdrop's footprint, so a
//!   barrier of 0 there means nothing lies under it.

use crate::cascade::plane_backdrop::PlaneBackdrop;
use crate::damage::Damage;
use crate::damage::dirty_backdrops::DirtyBackdrops;
use crate::display::Display;
use crate::primitives::geometry::urect::URect;
use crate::primitives::identity::backdrop_key::BackdropKey;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::backdrop_row::BackdropCapture;
use crate::renderer::render_plan::RenderPlan;
use crate::shape::paint::blur_plan::BlurPlan;
use glam::UVec2;
use rustc_hash::FxHashMap;

/// What a backdrop's held capture blurred: its plan, its surface and its region in texels of the
/// plan's last level.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Held {
    plan: BlurPlan,
    surface: UVec2,
    region: URect,
}

impl Held {
    /// Whether this capture serves a backdrop of `plan` at `region` on `surface`: blurred alike
    /// over a region that holds it. The backdrop's texels then depend only on texels inside the
    /// held one.
    fn serves(&self, plan: BlurPlan, region: URect, surface: UVec2) -> bool {
        self.plan == plan && self.surface == surface && self.region.union(region) == self.region
    }
}

/// What a frame does with one backdrop, decided before it encodes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BackdropTicket {
    pub(crate) capture: BackdropCapture,
    /// Whether the backdrop takes a new result this frame, from a capture or the clear.
    pub(crate) captures: bool,
}

/// What a frame's damage left of its backdrops' captures.
#[derive(Clone, Copy, Debug)]
pub(crate) enum BackdropDamage<'a> {
    /// A whole frame: every backdrop captures.
    Whole,
    /// A partial frame: the backdrops `dirty` holds are stale.
    Stale(&'a DirtyBackdrops),
}

impl<'a> BackdropDamage<'a> {
    /// The backdrops a frame painted by `plan` left stale, `dirty` where it is partial.
    pub(crate) const fn of(plan: RenderPlan, dirty: &'a DirtyBackdrops) -> Self {
        match plan.damage {
            Damage::Full => Self::Whole,
            Damage::Partial(_) => Self::Stale(dirty),
        }
    }

    /// Whether a backdrop keyed `key` may keep what it holds.
    fn keeps(self, key: BackdropKey) -> bool {
        match self {
            Self::Whole => false,
            Self::Stale(dirty) => !dirty.contains(key),
        }
    }
}

/// Whether a frame's captures fit a partial repaint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Settled {
    /// Every capturing backdrop's footprint joins the repaint scissors.
    Fits,
    /// More backdrops capture than a partial repaint holds scissors for: the frame paints whole,
    /// and these tickets do not serve it.
    Overflow,
}

/// One window's backdrop ledger: the captures its backdrops hold, and this frame's
/// [`BackdropTicket`]s. The backend keeps the results and follows each row's
/// [`source`](crate::renderer::render_buffer::backdrop_row::BackdropRow::source).
#[derive(Debug, Default)]
pub(crate) struct BackdropCaptures {
    held: FxHashMap<BackdropKey, Held>,
    /// A ticket for each backdrop [`Self::decide`] took last; `None` for a backdrop whose footprint
    /// misses the surface, which paints nothing.
    tickets: Vec<Option<BackdropTicket>>,
}

impl BackdropCaptures {
    /// The most backdrops a partial frame captures; past it the frame paints whole.
    pub(crate) const PARTIAL_CAP: usize = DirtyBackdrops::CAP;

    /// Write a ticket for each of `backdrops`, a frame's in paint order on `display`. Every
    /// backdrop captures on a whole frame, else each one `damage` left stale or whose held capture
    /// does not serve. When more capture than a partial repaint holds, [`Settled::Overflow`] says
    /// the frame paints whole, and the caller decides again for it. Holds nothing new:
    /// [`Self::settle`] records what the built frame took.
    pub(crate) fn decide<'a>(
        &mut self,
        backdrops: impl IntoIterator<Item = &'a PlaneBackdrop>,
        display: Display,
        damage: BackdropDamage<'_>,
    ) -> Settled {
        let surface = display.physical;
        self.tickets.clear();
        let mut captures = 0;
        for backdrop in backdrops {
            let footprint = backdrop.physical_footprint(display);
            let ticket = backdrop.plan.region(footprint, surface).map(|region| {
                let held = damage
                    .keeps(backdrop.key)
                    .then(|| self.held.get(&backdrop.key))
                    .flatten()
                    .filter(|held| held.serves(backdrop.plan, region, surface));
                captures += usize::from(held.is_none());
                BackdropTicket {
                    capture: BackdropCapture {
                        key: backdrop.key,
                        footprint,
                        plan: backdrop.plan,
                        region: held.map_or(region, |held| held.region),
                    },
                    captures: held.is_none(),
                }
            });
            self.tickets.push(ticket);
        }
        if matches!(damage, BackdropDamage::Whole) || captures <= Self::PARTIAL_CAP {
            Settled::Fits
        } else {
            Settled::Overflow
        }
    }

    /// Stop holding backdrop `key`'s capture, whose result the device no longer keeps.
    pub(crate) fn forget(&mut self, key: BackdropKey) {
        self.held.remove(&key);
    }

    /// The tickets [`Self::decide`] wrote last.
    pub(crate) fn tickets(&self) -> &[Option<BackdropTicket>] {
        &self.tickets
    }

    /// Whether a backdrop of the last tickets may capture: the frame built from them captures at
    /// most these.
    pub(crate) fn may_capture(&self) -> bool {
        self.capturing().next().is_some()
    }

    /// The physical footprint of each backdrop the last tickets may capture.
    pub(crate) fn capture_footprints(&self) -> impl Iterator<Item = URect> + '_ {
        self.capturing().map(|ticket| ticket.capture.footprint)
    }

    fn capturing(&self) -> impl Iterator<Item = &BackdropTicket> {
        self.tickets
            .iter()
            .flatten()
            .filter(|ticket| ticket.captures)
    }

    /// Record what the frame `buffer` holds, built from the last tickets, took: each backdrop that
    /// took a new result holds it, each capturing backdrop that took none holds nothing, and a
    /// backdrop the frame no longer has, which its [`live_backdrops`](RenderBuffer::live_backdrops)
    /// leave out, drops its own.
    pub(crate) fn settle(&mut self, buffer: &RenderBuffer) {
        let live = &buffer.live_backdrops;
        self.held.retain(|&key, _| live.contains(key));
        for ticket in self
            .tickets
            .iter()
            .flatten()
            .filter(|ticket| ticket.captures)
        {
            self.held.remove(&ticket.capture.key);
        }
        let surface = buffer.display.physical;
        for row in &buffer.backdrops {
            if row.source.is_new() {
                let BackdropCapture {
                    key, plan, region, ..
                } = row.capture;
                self.held.insert(
                    key,
                    Held {
                        plan,
                        surface,
                        region,
                    },
                );
            }
        }
    }
}

#[cfg(test)]
mod tests;
