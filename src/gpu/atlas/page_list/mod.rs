//! [`PageList`]: the pages of one texel format, the items packed on them, and the textures of
//! their own it hands items too large for a page.

use crate::common::expiry_window::EXPIRY_FRAMES;
use crate::gpu::atlas::Evictions;
use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::atlas::atlas_handle::{AtlasHandle, Placement};
use crate::gpu::atlas::error::AtlasError;
use crate::gpu::atlas::eviction_clock::{ClockSweep, Verdict};
use crate::gpu::atlas::scratch_cache::{ScratchCache, ScratchId};
use crate::gpu::atlas::tenant::Tenant;
use crate::gpu::resource::shelf_packer::ShelfPacker;
use glam::{U16Vec2, UVec2};

/// The pages a list binds together, Skia's `kMaxMultitexturePages`: its hard cap.
pub(crate) const MAX_PAGES: usize = 4;

/// What a caller sets of a list (`.notes/proposals/atlas-pool.md`, Starting constants).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PageListConfig {
    /// The edge a list's first page opens at, a power of two.
    pub(crate) first_edge: u32,
    /// The edge a page doubles to and every later page opens at, a power of two.
    pub(crate) page_edge: u32,
    /// The bytes of pages and scratch past which the list evicts before it grows.
    pub(crate) soft_bytes: u64,
}

/// What a page takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PageState {
    /// Allocations, and a drain's moves into it.
    Open,
    /// A drain's source: no allocation and no move into it, so it empties and goes, and is never a
    /// source and a destination in one frame.
    Draining,
}

/// One item a drain moved, from the page of opening `from_epoch` to another, which the GPU half
/// copies at the next commit, ahead of every upload, bake and draw: its handle stays, and its
/// tenant reads the new placement at its next lookup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Move {
    pub(crate) from: Placement,
    pub(crate) from_epoch: u32,
    pub(crate) to: Placement,
}

/// What a display frame's moves may still take, across the lists: WebRender's compaction
/// `area_threshold` of texels, 512², and a count of copy commands, as each costs its driver's
/// fixed overhead (2.8 µs on an Apple laptop, `.notes/research/2026-11-12-atlas-pool-measurements.md`),
/// so 64 hold a frame's moves near 0.2 ms there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MoveBudget {
    pub(crate) texels: u64,
    pub(crate) copies: u32,
}

impl MoveBudget {
    /// One display frame's.
    pub(crate) const FRAME: Self = Self {
        texels: 512 * 512,
        copies: 64,
    };

    /// Take `texels` in one copy, `false` where the budget holds them no more.
    const fn take(&mut self, texels: u64) -> bool {
        if self.copies == 0 || self.texels < texels {
            return false;
        }
        self.copies -= 1;
        self.texels -= texels;
        true
    }
}

/// When a drain plans: in a display frame's decay, where a sparse page shrinks only after an
/// expiry window of frames, or in a purge, where it shrinks at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pace {
    Frame(u64),
    Purge,
}

/// What a list's decay writes into, beside the list.
#[derive(Debug)]
pub(crate) struct DecayCtx<'a> {
    pub(crate) budget: &'a mut MoveBudget,
    pub(crate) evictions: &'a mut Evictions,
    pub(crate) moves: &'a mut Vec<Move>,
}

/// WebRender's age at which an item idle under a pressure of `ratio`, a list's bytes over its
/// soft budget in whole multiples, goes (`TextureCache::should_continue_evicting`): 400 frames
/// at none, halving with each multiple, 1 past six.
const fn age_threshold(ratio: u64) -> u64 {
    match ratio {
        0 => 400,
        1 => 200,
        2 => 100,
        3 => 50,
        4 => 25,
        5 => 10,
        6 => 5,
        _ => 1,
    }
}

/// The evictions one display frame's age-out takes below four times the soft budget, WebRender's
/// `MAX_EVICTIONS_PER_FRAME`, which keeps a frame of many from a stutter.
const AGE_EVICTIONS_MAX: u32 = 32;

/// The slab slots one display frame's age-out walks, from where the last stopped, so its cost a
/// frame holds however many items the list keeps; a slab of more takes more frames to walk.
const AGE_SCAN_MAX: usize = 1024;

/// One page: its packer, whose edge is the page's, what it takes, the texels its items fill, and
/// which opening of its slot it is.
#[derive(Debug)]
pub(crate) struct Page {
    packer: ShelfPacker,
    state: PageState,
    live_texels: u64,
    /// The list's count of pages opened when this one opened, so the GPU half tells a page grown
    /// in its slot, whose texels it copies, from another opened there since, whose it does not.
    epoch: u32,
}

impl Page {
    fn new(edge: u32, state: PageState, epoch: u32) -> Self {
        Self {
            packer: ShelfPacker::new(edge_u16(edge)),
            state,
            live_texels: 0,
            epoch,
        }
    }

    pub(crate) const fn epoch(&self) -> u32 {
        self.epoch
    }

    pub(crate) fn edge(&self) -> u32 {
        u32::from(self.packer.edge())
    }

    const fn takes_allocations(&self) -> bool {
        !matches!(self.state, PageState::Draining)
    }

    /// Whether an item of `size` fits inside the page's edge at all.
    fn holds(&self, size: U16Vec2) -> bool {
        u32::from(size.max_element()) <= self.edge()
    }
}

/// One slot of the slab: what it holds, the generation a handle to it carries, the display frame
/// it was last drawn in, the pins on it and its tenant.
#[derive(Clone, Copy, Debug)]
struct Entry {
    placement: Option<Placement>,
    generation: u32,
    last_use: u64,
    pins: u32,
    tenant: Tenant,
}

impl Entry {
    /// Whether no eviction may take it at display frame `frame`: pinned, or drawn this frame or
    /// the last, the one-frame grace that keeps a working set a little past budget from evicting
    /// what the next frame draws again, as WebRender's least age at full pressure.
    const fn is_exempt(&self, frame: u64) -> bool {
        self.pins > 0 || self.last_use + 1 >= frame
    }
}

/// What a shelf holds, as [`PageList::evict_shelf`] tallies it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShelfUse {
    Empty,
    /// Items, every one evictable.
    Evictable,
    /// An item no eviction may take.
    Kept,
}

/// Which bytes a grow may reach.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Budget {
    /// The list's soft budget, under which it grows rather than evicts.
    Soft,
    /// [`MAX_PAGES`] pages at the page edge, once nothing is evictable.
    Hard,
}

/// The pages of one format, at most [`MAX_PAGES`], their items in a slab, and a [`ScratchCache`].
///
/// An insert places the item; failing that, under the soft budget the list grows, doubling a page
/// below the page edge or opening a page; over it, it evicts, by a clock over the items no
/// frame drew this frame or the last and no one pins; with nothing evictable it grows to its hard
/// cap; at the cap it refuses. As the raster atlas inserts today, across pages
/// (`.notes/proposals/atlas-pool.md`, Packed pages).
#[derive(Debug)]
pub(crate) struct PageList {
    format: AtlasFormat,
    config: PageListConfig,
    /// By page index, which a placement and a draw name.
    pages: [Option<Page>; MAX_PAGES],
    /// Pages opened so far, the next one's [`Page::epoch`].
    opened: u32,
    entries: Vec<Entry>,
    /// Slab slots no item holds, the next insert's.
    free: Vec<u32>,
    /// Where the clock resumes.
    hand: u32,
    /// The display frame a whole turn of the clock found nothing evictable in: every item stays
    /// exempt for the rest of that frame, so no insert walks the slab again.
    dry_frame: Option<u64>,
    /// [`Self::evict_shelf`]'s tally, kept so a starving insert allocates nothing.
    shelf_uses: Vec<ShelfUse>,
    scratch: ScratchCache,
    /// The device's largest texture edge, which no scratch texture passes.
    device_edge: u32,
    /// Where the age-out resumes.
    age_hand: u32,
    /// The display frame since which the list's one page has held under a quarter of its texels,
    /// `None` while it holds more or the list has other pages.
    sparse_since: Option<u64>,
    /// The last drain that found no room for an item, so the same drain is not tried again
    /// every frame while nothing it counts changed: the free texels fit its items, but not
    /// their shapes.
    stalled: Option<DrainCount>,
}

/// What a drain of a page counts: the page, by opening, the texels its items fill and the free
/// texels of the others.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DrainCount {
    epoch: u32,
    live_texels: u64,
    others_free: u64,
}

impl PageList {
    /// An empty list of `format` as `config` sets it, its edges held to `device_edge`.
    ///
    /// # Panics
    ///
    /// If an edge of `config` is not a power of two, the first passes the page edge, or the page
    /// edge passes what a packer counts.
    pub(crate) fn new(format: AtlasFormat, config: PageListConfig, device_edge: u32) -> Self {
        assert!(
            config.first_edge.is_power_of_two(),
            "a first page edge of {} is no power of two",
            config.first_edge
        );
        assert!(
            config.page_edge.is_power_of_two(),
            "a page edge of {} is no power of two",
            config.page_edge
        );
        assert!(
            config.first_edge <= config.page_edge,
            "a first page of {} passes the page edge of {}",
            config.first_edge,
            config.page_edge
        );
        assert!(
            config.page_edge <= ShelfPacker::MAX_SIZE,
            "a page edge of {} passes the packer's",
            config.page_edge
        );
        assert!(device_edge > 0, "a device holds a texel");
        let device_pow2 = 1 << device_edge.ilog2();
        let page_edge = config.page_edge.min(device_pow2);
        Self {
            format,
            config: PageListConfig {
                first_edge: config.first_edge.min(page_edge),
                page_edge,
                ..config
            },
            pages: [const { None }; MAX_PAGES],
            opened: 0,
            entries: Vec::new(),
            free: Vec::new(),
            hand: 0,
            dry_frame: None,
            shelf_uses: Vec::new(),
            scratch: ScratchCache::default(),
            device_edge,
            age_hand: 0,
            sparse_since: None,
            stalled: None,
        }
    }

    /// The page at `idx`, `None` where none is open.
    pub(crate) const fn page(&self, idx: usize) -> Option<&Page> {
        self.pages[idx].as_ref()
    }

    /// The bytes of the list's pages and scratch textures.
    pub(crate) fn bytes(&self) -> u64 {
        let pages: u64 = self
            .pages
            .iter()
            .flatten()
            .map(|page| u64::from(page.edge()).pow(2))
            .sum();
        (pages + self.scratch.texels()) * u64::from(self.format.texel_bytes())
    }

    /// Whether an item of `size` is better a texture of its own, wider or taller than half a page,
    /// as WebRender's `is_allowed_in_shared_cache`: what a per-draw tenant checks.
    pub(crate) fn prefers_scratch(&self, size: UVec2) -> bool {
        size.max_element() > self.config.page_edge / 2
    }

    /// Room for an item of `size` for `tenant` at display frame `frame`, evicting into
    /// `evictions` as the insert needs.
    pub(crate) fn allocate(
        &mut self,
        tenant: Tenant,
        size: U16Vec2,
        frame: u64,
        evictions: &mut Evictions,
    ) -> Result<AtlasHandle, AtlasError> {
        let edge = self.config.page_edge;
        if u32::from(size.max_element()) > edge {
            return Err(AtlasError::Oversized {
                size: size.as_uvec2(),
                edge,
            });
        }
        assert!(size.x > 0 && size.y > 0, "an item holds a texel");
        let placement = loop {
            if let Some(placement) = self.place(size) {
                break placement;
            }
            // The drain's source takes the item rather than the list grow beside it.
            if self.cancel_drain() {
                continue;
            }
            if self.grow(size, Budget::Soft) {
                continue;
            }
            if self.holds_now(size) && self.evict_for(size, frame, evictions) > 0 {
                continue;
            }
            if self.grow(size, Budget::Hard) {
                continue;
            }
            return Err(AtlasError::Full);
        };
        Ok(self.store(tenant, placement, frame))
    }

    /// The rectangle of the first page that takes allocations and has room for `size`.
    fn place(&mut self, size: U16Vec2) -> Option<Placement> {
        for (idx, page) in self.pages.iter_mut().enumerate() {
            let Some(page) = page.as_mut().filter(|page| page.takes_allocations()) else {
                continue;
            };
            if let Some(origin) = page.packer.allocate(size) {
                let placement = Placement {
                    page: idx as u8,
                    origin,
                    size,
                };
                page.live_texels += placement.texels();
                return Some(placement);
            }
        }
        None
    }

    /// Whether a page that takes allocations holds `size` at its edge now, so an eviction can
    /// make room for it.
    fn holds_now(&self, size: U16Vec2) -> bool {
        self.pages
            .iter()
            .flatten()
            .any(|page| page.takes_allocations() && page.holds(size))
    }

    /// Grow within `budget`: double the first page below the page edge that takes allocations,
    /// else open a page, at the first edge where the list has none and the page edge after; `false`
    /// where the budget or the slots refuse.
    fn grow(&mut self, size: U16Vec2, budget: Budget) -> bool {
        let limit = match budget {
            Budget::Soft => self.config.soft_bytes,
            Budget::Hard => u64::MAX,
        };
        let texel = u64::from(self.format.texel_bytes());
        let bytes = self.bytes();
        let page_edge = self.config.page_edge;
        if let Some(page) = self
            .pages
            .iter_mut()
            .flatten()
            .find(|page| page.takes_allocations() && page.edge() < page_edge)
        {
            let edge = page.edge();
            let added = (u64::from(edge * 2).pow(2) - u64::from(edge).pow(2)) * texel;
            if bytes + added > limit {
                return false;
            }
            page.packer.grow(edge_u16(edge * 2));
            return true;
        }
        let Some(slot) = self.pages.iter().position(Option::is_none) else {
            return false;
        };
        let edge = if self.pages.iter().all(Option::is_none) {
            self.config.first_edge
        } else {
            page_edge
        };
        debug_assert!(
            u32::from(size.max_element()) <= page_edge,
            "an item past the page edge was refused before any grow"
        );
        if bytes + u64::from(edge).pow(2) * texel > limit {
            return false;
        }
        self.pages[slot] = Some(Page::new(edge, PageState::Open, self.opened));
        self.opened = self.opened.checked_add(1).expect("a list opened 2³² pages");
        true
    }

    /// Evict toward room for `size` at display frame `frame`: an evictable item on a shelf that
    /// takes it, chosen by the clock; with none, every item on one shelf, so its rows serve
    /// another height. How many went, `0` where none could.
    fn evict_for(&mut self, size: U16Vec2, frame: u64, evictions: &mut Evictions) -> u32 {
        if self.dry_frame == Some(frame) {
            return 0;
        }
        let Self { entries, pages, .. } = &*self;
        let sweep = ClockSweep::over(entries.len(), self.hand, |idx| {
            let entry = &entries[idx];
            let Some(placement) = entry.placement else {
                return Verdict::Skip;
            };
            let page = pages[placement.page as usize]
                .as_ref()
                .expect("an item sits on an open page");
            if !page.takes_allocations() || entry.is_exempt(frame) {
                return Verdict::Skip;
            }
            let shelf = page.packer.shelf_height(placement.origin.y);
            if page.holds(size) && ShelfPacker::takes(shelf, size.y) {
                Verdict::Take
            } else {
                Verdict::Refuse
            }
        });
        self.hand = sweep.hand;
        if let Some(idx) = sweep.victim {
            self.evict(idx as usize, evictions);
            return 1;
        }
        if !sweep.refused {
            self.dry_frame = Some(frame);
            return 0;
        }
        self.evict_shelf(size, frame, evictions)
    }

    /// With no victim on a shelf that takes `size`, every item of one shelf whose items are all
    /// evictable, on a page that takes allocations and holds `size`: the shortest tall enough,
    /// for the item to reuse, else a page's bottom shelf, whose rows go back to its page. `0`
    /// where no shelf qualifies, as emptying a short one in the middle frees nothing usable.
    fn evict_shelf(&mut self, size: U16Vec2, frame: u64, evictions: &mut Evictions) -> u32 {
        let mut tall: Option<(u16, usize, u16)> = None;
        let mut bottom: Option<(usize, u16)> = None;
        for (page_idx, page) in self.pages.iter().enumerate() {
            let Some(page) = page
                .as_ref()
                .filter(|page| page.takes_allocations() && page.holds(size))
            else {
                continue;
            };
            let shelves = page.packer.shelves();
            let uses = &mut self.shelf_uses;
            uses.clear();
            uses.resize(shelves.len(), ShelfUse::Empty);
            for entry in &self.entries {
                let Some(placement) = entry.placement.filter(|p| p.page as usize == page_idx)
                else {
                    continue;
                };
                let shelf_use = &mut uses[page.packer.shelf_at(placement.origin.y)];
                if entry.is_exempt(frame) {
                    *shelf_use = ShelfUse::Kept;
                } else if *shelf_use == ShelfUse::Empty {
                    *shelf_use = ShelfUse::Evictable;
                }
            }
            for (shelf, &shelf_use) in shelves.iter().zip(uses.iter()) {
                if shelf_use == ShelfUse::Evictable
                    && shelf.height >= size.y
                    && tall.is_none_or(|(height, ..)| shelf.height < height)
                {
                    tall = Some((shelf.height, page_idx, shelf.y));
                }
            }
            if bottom.is_none() && uses.last() == Some(&ShelfUse::Evictable) {
                let last = shelves.last().expect("a tally a shelf");
                bottom = Some((page_idx, last.y));
            }
        }
        let Some((page_idx, shelf_y)) = tall.map(|(_, page, y)| (page, y)).or(bottom) else {
            return 0;
        };
        let mut evicted = 0;
        for idx in 0..self.entries.len() {
            let on_shelf = self.entries[idx]
                .placement
                .is_some_and(|p| p.page as usize == page_idx && p.origin.y == shelf_y);
            if on_shelf {
                self.evict(idx, evictions);
                evicted += 1;
            }
        }
        evicted
    }

    /// Take the item at slab slot `idx` and queue its handle for its tenant.
    fn evict(&mut self, idx: usize, evictions: &mut Evictions) {
        let entry = self.entries[idx];
        evictions.push(entry.tenant, self.handle_at(idx));
        self.free_slot(idx);
    }

    /// File `placement` for `tenant` at display frame `frame` in a free slot, its generation
    /// kept, else a new one.
    fn store(&mut self, tenant: Tenant, placement: Placement, frame: u64) -> AtlasHandle {
        let entry = |generation| Entry {
            placement: Some(placement),
            generation,
            last_use: frame,
            pins: 0,
            tenant,
        };
        let idx = if let Some(idx) = self.free.pop() {
            let slot = &mut self.entries[idx as usize];
            *slot = entry(slot.generation);
            idx as usize
        } else {
            self.entries.push(entry(0));
            self.entries.len() - 1
        };
        self.handle_at(idx)
    }

    fn handle_at(&self, idx: usize) -> AtlasHandle {
        AtlasHandle {
            format: self.format,
            idx: u32::try_from(idx).expect("a slab counts its slots in u32"),
            generation: self.entries[idx].generation,
        }
    }

    /// Give the item at `idx` back: its rectangle to its page, its slot to the free list, its
    /// generation moved so every handle to it reads as stale.
    fn free_slot(&mut self, idx: usize) {
        let entry = &mut self.entries[idx];
        let placement = entry.placement.take().expect("a held slot frees once");
        entry.generation = entry
            .generation
            .checked_add(1)
            .expect("a slot generation overflowed");
        let page = self.pages[placement.page as usize]
            .as_mut()
            .expect("an item sits on an open page");
        page.packer.deallocate(placement.origin, placement.size.x);
        page.live_texels -= placement.texels();
        self.free
            .push(u32::try_from(idx).expect("a slab counts its slots in u32"));
    }

    /// The slot `handle` names, where it still holds the item.
    fn live(&self, handle: AtlasHandle) -> Option<usize> {
        debug_assert_eq!(handle.format, self.format, "a handle of another list");
        let idx = handle.idx as usize;
        let entry = self.entries.get(idx)?;
        (entry.generation == handle.generation && entry.placement.is_some()).then_some(idx)
    }

    /// Where `handle`'s item sits, `None` once it went.
    pub(crate) fn placement(&self, handle: AtlasHandle) -> Option<Placement> {
        let idx = self.live(handle)?;
        self.entries[idx].placement
    }

    /// Mark `handle`'s item drawn at display frame `frame`, so no eviction takes it this frame or
    /// the next, and say where it sits; `None` once it went.
    pub(crate) fn stamp(&mut self, handle: AtlasHandle, frame: u64) -> Option<Placement> {
        let idx = self.live(handle)?;
        let entry = &mut self.entries[idx];
        debug_assert!(entry.last_use <= frame, "the display frames ran backwards");
        entry.last_use = frame;
        entry.placement
    }

    /// Give `handle`'s item back.
    ///
    /// # Panics
    ///
    /// If the item went already, or a pin holds it.
    pub(crate) fn release(&mut self, handle: AtlasHandle) {
        let idx = self.live(handle).expect("a released item is held");
        assert_eq!(self.entries[idx].pins, 0, "a pinned item released");
        self.free_slot(idx);
    }

    /// Keep `handle`'s item from every eviction until a matching [`Self::unpin`].
    ///
    /// # Panics
    ///
    /// If the item went.
    pub(crate) fn pin(&mut self, handle: AtlasHandle) {
        let idx = self.live(handle).expect("a pinned item is held");
        self.entries[idx].pins += 1;
    }

    /// Take back one pin of `handle`'s item; it may be evictable at once, so the dry latch opens.
    ///
    /// # Panics
    ///
    /// If the item went or holds no pin.
    pub(crate) fn unpin(&mut self, handle: AtlasHandle) {
        let idx = self.live(handle).expect("an unpinned item is held");
        let pins = &mut self.entries[idx].pins;
        assert!(*pins > 0, "an item unpinned more than pinned");
        *pins -= 1;
        self.dry_frame = None;
    }

    /// Set what page `idx` takes, as a drain begins and ends; the dry latch opens, as a page that
    /// takes allocations again brings its items back to the clock.
    ///
    /// # Panics
    ///
    /// If no page is open at `idx`.
    pub(crate) const fn set_state(&mut self, idx: usize, state: PageState) {
        self.pages[idx].as_mut().expect("an open page").state = state;
        self.dry_frame = None;
    }

    /// Close page `idx`, which holds no item, so its slot takes the next page opened.
    ///
    /// # Panics
    ///
    /// If no page is open at `idx`, or an item sits on it.
    pub(crate) fn close_page(&mut self, idx: usize) {
        let page = self.pages[idx].take().expect("an open page");
        assert_eq!(page.live_texels, 0, "a page closed under its items");
    }

    pub(crate) const fn scratch(&self) -> &ScratchCache {
        &self.scratch
    }

    /// A texture of its own for an item of `size`.
    pub(crate) fn acquire_scratch(&mut self, size: UVec2) -> Result<ScratchId, AtlasError> {
        self.scratch.acquire(size, self.device_edge)
    }

    /// Give back the texture `id` holds at display frame `frame`.
    pub(crate) fn release_scratch(&mut self, id: ScratchId, frame: u64) {
        self.scratch.release(id, frame);
    }

    /// Drop the scratch textures idle for an expiry window at display frame `frame`.
    pub(crate) fn expire(&mut self, frame: u64) {
        self.scratch.expire(frame);
    }

    /// Return what the list holds past its need, once a display frame, at display frame `frame`
    /// (`.notes/proposals/atlas-pool.md`, Decay): age out what idled past the age the list's
    /// pressure sets into `evictions`; with one page under a quarter full for an expiry window,
    /// open a smaller one to drain into; with several, drain the emptiest whose items fit the
    /// others'; move a drain's items within `budget` into `moves`; and close the page it emptied.
    pub(crate) fn decay(&mut self, frame: u64, ctx: DecayCtx<'_>) {
        let DecayCtx {
            budget,
            evictions,
            moves,
        } = ctx;
        self.age_out(frame, evictions);
        if !self.is_draining() && moves.is_empty() {
            self.plan_drain(Pace::Frame(frame));
        }
        self.drain(budget, moves);
    }

    /// Whether a page is draining.
    fn is_draining(&self) -> bool {
        self.pages
            .iter()
            .flatten()
            .any(|page| !page.takes_allocations())
    }

    /// Give back all the list can at once, as the system runs short of memory, at display frame
    /// `frame`: evict every item no one pins and this frame did not draw into `evictions`, drop the
    /// idle scratch, and drain with no budget, a sparse page shrinking at once, into `moves`, until
    /// no drain is left to start. A stutter traded for the process not being killed
    /// (`.notes/proposals/atlas-pool.md`, Decay).
    pub(crate) fn purge(&mut self, frame: u64, evictions: &mut Evictions, moves: &mut Vec<Move>) {
        for idx in 0..self.entries.len() {
            let entry = &self.entries[idx];
            if entry.placement.is_some() && entry.pins == 0 && entry.last_use < frame {
                self.evict(idx, evictions);
            }
        }
        self.dry_frame = None;
        self.scratch.purge();
        let mut budget = MoveBudget {
            texels: u64::MAX,
            copies: u32::MAX,
        };
        // A drain ends with its page closed or stalled, so each round closes a page or stops: the
        // rounds are at most the pages, one more for a shrink's new page. A round that moved an
        // item ends the purge, as the next drain could take the page it landed on.
        for _ in 0..=MAX_PAGES {
            if !self.is_draining() {
                if !moves.is_empty() {
                    break;
                }
                self.plan_drain(Pace::Purge);
            }
            if !self.is_draining() {
                break;
            }
            self.drain(&mut budget, moves);
        }
    }

    /// Evict the items no one pins and no frame drew for WebRender's age at the list's pressure,
    /// at most [`AGE_EVICTIONS_MAX`] below four times the soft budget, walking at most
    /// [`AGE_SCAN_MAX`] slots. Only past the budget or past the list's first page, as WebRender
    /// ages its cache only past its threshold or across several textures: a list at its first
    /// page under budget keeps what it holds.
    fn age_out(&mut self, frame: u64, evictions: &mut Evictions) {
        let bytes = self.bytes();
        let soft = self.config.soft_bytes;
        if bytes < soft && !self.is_past_first_page() {
            return;
        }
        let ratio = bytes.checked_div(soft).unwrap_or(u64::MAX);
        let age = age_threshold(ratio);
        let evictions_max = if soft == 0 || bytes > 4 * soft {
            u32::MAX
        } else {
            AGE_EVICTIONS_MAX
        };
        let len = self.entries.len();
        let mut evicted = 0;
        for _ in 0..len.min(AGE_SCAN_MAX) {
            if evicted == evictions_max {
                break;
            }
            let idx = self.age_hand as usize % len;
            self.age_hand = u32::try_from((idx + 1) % len).expect("a slab counts its slots in u32");
            let entry = &self.entries[idx];
            // Idle past the age, and outside the grace: the last frame is `frame - 1`.
            if entry.placement.is_some() && entry.pins == 0 && entry.last_use + age < frame {
                self.evict(idx, evictions);
                evicted += 1;
            }
        }
        if evicted > 0 {
            self.dry_frame = None;
        }
    }

    /// Whether the list holds more than its first page at its first edge.
    fn is_past_first_page(&self) -> bool {
        let mut pages = self.pages.iter().flatten();
        match (pages.next(), pages.next()) {
            (Some(page), None) => page.edge() > self.config.first_edge,
            (Some(_), Some(_)) => true,
            (None, _) => false,
        }
    }

    /// Start a drain at `pace`, where no move waits for the commit, so no page a move lands on
    /// drains before its texture takes the move: with one page under a quarter full for an expiry
    /// window, or at once in a purge, past the first edge, a page at the smallest power-of-two edge from the first that
    /// holds twice its items' texels opens beside it, and it drains into that; with several pages,
    /// the emptiest whose items fit half the others' free texels drains, so packing slack leaves
    /// room. Neither where no page is open, or the list has no free slot.
    fn plan_drain(&mut self, pace: Pace) {
        let open = self.pages.iter().flatten().count();
        if open == 1 {
            let (idx, page) = self
                .pages
                .iter()
                .enumerate()
                .find_map(|(idx, page)| page.as_ref().map(|page| (idx, page)))
                .expect("one page is open");
            let area = u64::from(page.edge()).pow(2);
            let sparse = page.edge() > self.config.first_edge && page.live_texels * 4 < area;
            if !sparse {
                self.sparse_since = None;
                return;
            }
            if let Pace::Frame(frame) = pace {
                let since = *self.sparse_since.get_or_insert(frame);
                if frame - since + 1 < EXPIRY_FRAMES {
                    return;
                }
            }
            self.sparse_since = None;
            let mut edge = self.config.first_edge;
            while u64::from(edge).pow(2) < 2 * page.live_texels {
                edge *= 2;
            }
            if edge >= page.edge() {
                return;
            }
            let slot = self
                .pages
                .iter()
                .position(Option::is_none)
                .expect("a list of one page has free slots");
            self.pages[slot] = Some(Page::new(edge, PageState::Open, self.opened));
            self.opened = self.opened.checked_add(1).expect("a list opened 2³² pages");
            self.set_state(idx, PageState::Draining);
            return;
        }
        self.sparse_since = None;
        if open < 2 {
            return;
        }
        let emptiest = self
            .pages
            .iter()
            .enumerate()
            .filter_map(|(idx, page)| page.as_ref().map(|page| (idx, page.live_texels)))
            .min_by_key(|&(_, live)| live)
            .map(|(idx, _)| idx);
        let Some(idx) = emptiest else {
            return;
        };
        let count = self.drain_count(idx);
        if count.live_texels * 2 <= count.others_free && self.stalled != Some(count) {
            self.set_state(idx, PageState::Draining);
        }
    }

    /// What a drain of page `idx`, which is open, counts.
    fn drain_count(&self, idx: usize) -> DrainCount {
        let free = |page: &Page| u64::from(page.edge()).pow(2) - page.live_texels;
        let page = self.pages[idx].as_ref().expect("an open page");
        DrainCount {
            epoch: page.epoch,
            live_texels: page.live_texels,
            others_free: self.pages.iter().flatten().map(free).sum::<u64>() - free(page),
        }
    }

    /// Move the draining page's items onto the others within `budget`, each placed as an insert
    /// would, and close the page once it holds none. A move that finds no room cancels the drain
    /// and stalls it until what it counts changes.
    fn drain(&mut self, budget: &mut MoveBudget, moves: &mut Vec<Move>) {
        let Some(source) = self
            .pages
            .iter()
            .position(|page| page.as_ref().is_some_and(|page| !page.takes_allocations()))
        else {
            return;
        };
        let from_epoch = self.pages[source].as_ref().expect("found above").epoch;
        for idx in 0..self.entries.len() {
            let Some(from) = self.entries[idx]
                .placement
                .filter(|placement| usize::from(placement.page) == source)
            else {
                continue;
            };
            if !budget.take(from.texels()) {
                break;
            }
            let Some(to) = self.place(from.size) else {
                self.stalled = Some(self.drain_count(source));
                self.cancel_drain();
                return;
            };
            let page = self.pages[source].as_mut().expect("the source is open");
            page.packer.deallocate(from.origin, from.size.x);
            page.live_texels -= from.texels();
            self.entries[idx].placement = Some(to);
            moves.push(Move {
                from,
                from_epoch,
                to,
            });
        }
        if self.pages[source]
            .as_ref()
            .is_some_and(|page| page.live_texels == 0)
        {
            self.close_page(source);
        }
    }

    /// Give a draining page its allocations back, `true` where one was draining.
    fn cancel_drain(&mut self) -> bool {
        let Some(idx) = self
            .pages
            .iter()
            .position(|page| page.as_ref().is_some_and(|page| !page.takes_allocations()))
        else {
            return false;
        };
        self.set_state(idx, PageState::Open);
        true
    }
}

/// An edge as a packer counts it, which [`PageList::new`] holds to [`ShelfPacker::MAX_SIZE`].
fn edge_u16(edge: u32) -> u16 {
    u16::try_from(edge).expect("a page edge fits the packer's")
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    #[cfg(test)]
    use crate::gpu::atlas::atlas_handle::AtlasHandle;
    use crate::gpu::atlas::page_list::Page;
    #[cfg(test)]
    use crate::gpu::atlas::page_list::PageList;
    #[cfg(test)]
    use crate::gpu::atlas::page_list::{PageListConfig, PageState};
    #[cfg(test)]
    use crate::gpu::atlas::scratch_cache::ScratchId;
    #[cfg(test)]
    use glam::UVec2;

    impl Page {
        /// The texels its items fill.
        pub(crate) const fn live_texels(&self) -> u64 {
            self.live_texels
        }

        #[cfg(test)]
        pub(crate) const fn state(&self) -> PageState {
            self.state
        }
    }

    #[cfg(test)]
    impl PageList {
        pub(crate) const fn config(&self) -> PageListConfig {
            self.config
        }

        /// The size of the texture `id` holds, `None` once released.
        pub(crate) fn scratch_size(&self, id: ScratchId) -> Option<UVec2> {
            self.scratch.size(id)
        }

        /// The display frame `handle`'s item was last drawn in, `None` once it went.
        pub(crate) fn last_use(&self, handle: AtlasHandle) -> Option<u64> {
            self.live(handle).map(|idx| self.entries[idx].last_use)
        }

        /// The pins on `handle`'s item, `None` once it went.
        pub(crate) fn pins(&self, handle: AtlasHandle) -> Option<u32> {
            self.live(handle).map(|idx| self.entries[idx].pins)
        }
    }
}

#[cfg(test)]
mod tests;
