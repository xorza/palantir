//! [`PageList`]: the pages of one texel format, the items packed on them, and the textures of
//! their own it hands items too large for a page.

use crate::common::eviction_clock::{ClockSweep, Verdict};
use crate::common::expiry_window::EXPIRY_FRAMES;
use crate::gpu::atlas::Evictions;
use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::atlas::atlas_handle::{Allocation, AtlasHandle, Placement};
use crate::gpu::atlas::error::AtlasError;
use crate::gpu::atlas::page_slots::PageSlots;
use crate::gpu::atlas::scratch_cache::ScratchCache;
use crate::gpu::atlas::tenant::Tenant;
use crate::gpu::device::pass_traffic::PassTraffic;
use crate::gpu::resource::shelf_packer::ShelfPacker;
use glam::{U16Vec2, UVec2};

/// The pages a list binds together, Skia's `kMaxMultitexturePages`: its hard cap.
pub(crate) const MAX_PAGES: usize = 4;

/// What a caller sets of a list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PageListConfig {
    /// The edge a list's first page opens at, a power of two.
    pub(crate) first_edge: u32,
    /// The edge a page doubles to and every later page opens at, a power of two.
    pub(crate) page_edge: u32,
    /// The bytes of pages and scratch past which the list evicts before it grows.
    pub(crate) soft_bytes: u64,
}

impl PageListConfig {
    /// `format`'s starting constants: soft budgets the sum of the budgets the tenants held before
    /// the pool, page edges whose four pages are the cap one tenant held, until the bench hosts'
    /// memory study tunes them (`.notes/research/2026-11-12-atlas-pool-measurements.md`).
    pub(crate) const fn of(format: AtlasFormat) -> Self {
        match format {
            AtlasFormat::R8 => Self {
                first_edge: 256,
                page_edge: 2048,
                soft_bytes: 8 << 20,
            },
            AtlasFormat::Rgba8 => Self {
                first_edge: 256,
                page_edge: 1024,
                soft_bytes: 8 << 20,
            },
            AtlasFormat::Rgba16F => Self {
                first_edge: 256,
                page_edge: 1024,
                soft_bytes: 32 << 20,
            },
        }
    }
}

/// A drain under way, one a list at most: the page it empties, which takes no allocation and no
/// move into it, so it is never a source and a destination in one frame, and where its walk of the
/// slab resumes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Drain {
    page: u8,
    /// The slab slot the next moves resume at. No item sits on the page behind it, as the page
    /// takes none while it drains, so a frame's drain walks what it moves and not the slab again.
    hand: u32,
}

/// What [`PageList::shrink`] does with a list's one page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shrink {
    /// It holds no item, so it closes.
    Close,
    /// Its items drain into a new page of at least this edge.
    To(u32),
}

/// One item a drain moved, from the page of opening `from_epoch` to another, which the GPU half
/// moves at the next commit, ahead of every upload, bake and draw: its handle stays, and its
/// tenant reads the new placement at its next lookup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Move {
    pub(crate) from: Placement,
    pub(crate) from_epoch: u32,
    pub(crate) to: Placement,
}

/// What a display frame's moves may still take, across the lists: WebRender's compaction
/// `area_threshold` of texels, 512², and a count of moves, which on a device that moves by copy
/// commands each cost the driver's fixed overhead. The texels are spent, not fitted: the move that
/// passes them goes and ends the frame's moves, as WebRender's `try_compaction` counts a move
/// before it compares, so an item of more texels than a whole frame's still moves and its drain
/// ends. Refused: a move held to what is left, which kept such an item's page draining for good.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MoveBudget {
    pub(crate) texels: u64,
    pub(crate) moves: u32,
}

impl MoveBudget {
    /// A display frame's on a device that moves by copy commands: 16, as a copy costs 43 µs on
    /// V3D, so they hold a frame's moves near 0.7 ms there, where 64 took 2.4 ms
    /// (`.notes/research/2026-11-12-atlas-pool-measurements.md`, "Moves and pages").
    pub(crate) const COPIES: Self = Self {
        texels: 512 * 512,
        moves: 16,
    };

    /// A display frame's on a device that moves by one draw a destination page: the texels alone,
    /// as the rectangles ride in the draw's instances.
    pub(crate) const DRAWN: Self = Self {
        texels: 512 * 512,
        moves: u32::MAX,
    };

    /// A display frame's on a device whose passes move `traffic`: by copy commands where a pass
    /// moves its whole target, as a draw onto a page would load and store all of it, by draw
    /// elsewhere (`src/gpu/AGENTS.md`, Atlases).
    pub(crate) const fn of(traffic: PassTraffic) -> Self {
        match traffic {
            PassTraffic::WholeTarget => Self::COPIES,
            PassTraffic::Drawn => Self::DRAWN,
        }
    }

    /// Whether the frame's moves are spent: no move left, or the texels taken.
    const fn is_spent(&self) -> bool {
        self.moves == 0 || self.texels == 0
    }

    /// Take one move of `texels`, from a budget not spent.
    const fn spend(&mut self, texels: u64) {
        debug_assert!(!self.is_spent(), "a move from a spent budget");
        self.moves -= 1;
        self.texels = self.texels.saturating_sub(texels);
    }
}

/// WebRender's age at which an item idle under a pressure of `ratio`, the bytes a list's items fill
/// over its soft budget in whole multiples, goes (`TextureCache::should_continue_evicting`): 400 frames
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

/// One page: its packer, whose edge is the page's, the texels its items fill, which opening of its
/// slot it is, and what its last drain that found no room counted.
#[derive(Debug)]
pub(crate) struct Page {
    packer: ShelfPacker,
    live_texels: u64,
    /// The list's count of pages opened when this one opened, so the GPU half tells a page grown
    /// in its slot, whose texels it copies, from another opened there since, whose it does not.
    epoch: u32,
    /// What the page's last drain that found no room for an item counted, or its last shrink that
    /// no smaller page packs, so the same drain is not planned again while nothing it counts
    /// changed: the free texels fit its items, but not their shapes. A page's own, as another
    /// page's drain may still find room while this one's cannot.
    stalled: Option<DrainCount>,
}

impl Page {
    fn new(edge: u32, epoch: u32) -> Self {
        Self {
            packer: ShelfPacker::new(edge_u16(edge)),
            live_texels: 0,
            epoch,
            stalled: None,
        }
    }

    pub(crate) const fn epoch(&self) -> u32 {
        self.epoch
    }

    pub(crate) fn edge(&self) -> u32 {
        u32::from(self.packer.edge())
    }

    /// The texels the page holds, `edge²`.
    fn texels(&self) -> u64 {
        u64::from(self.edge()).pow(2)
    }

    /// Whether an item of `size` fits inside the page's edge at all.
    fn holds(&self, size: U16Vec2) -> bool {
        u32::from(size.max_element()) <= self.edge()
    }

    /// A rectangle of `size` on the page, which sits at `idx`, now taken; `None` where the page
    /// has no room.
    fn allocate(&mut self, idx: usize, size: U16Vec2) -> Option<Placement> {
        let placement = Placement {
            page: u8::try_from(idx).expect("a list holds few pages"),
            origin: self.packer.allocate(size)?,
            size,
        };
        self.live_texels += placement.texels();
        Some(placement)
    }

    /// Give `placement`'s rectangle back.
    fn deallocate(&mut self, placement: Placement) {
        self.packer.deallocate(placement.origin, placement.size.x);
        self.live_texels -= placement.texels();
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
    /// The frames past its last draw an item stays out of the clock's reach: one, so nothing
    /// drawn this frame or the last goes, the grace that keeps a working set a little past budget
    /// from evicting what the next frame draws again, as WebRender's least age at full pressure.
    const GRACE_FRAMES: u64 = 1;

    /// Whether no one pins it and no frame drew it in the `age` frames before display frame
    /// `frame`, nor in `frame`: what an eviction may take, the clock at an age of
    /// [`Self::GRACE_FRAMES`], the age-out at its pressure's, a purge at none.
    const fn is_idle(&self, frame: u64, age: u64) -> bool {
        self.pins == 0 && self.last_use + age < frame
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

/// The item [`PageList::evict_shelf`] last found no shelf for, in display frame `frame`: an item
/// as tall and as wide finds none either until a shelf can become evictable, so a starving frame
/// tallies the slab once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ShelfDry {
    frame: u64,
    size: U16Vec2,
}

impl ShelfDry {
    /// Whether an item of `size` at display frame `frame` finds no shelf either: no shorter and no
    /// narrower, as a taller item takes fewer shelves and a wider one fewer pages.
    fn covers(self, size: U16Vec2, frame: u64) -> bool {
        self.frame == frame
            && size.y >= self.size.y
            && size.max_element() >= self.size.max_element()
    }
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
/// (`src/gpu/AGENTS.md`, Atlases).
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
    /// exempt for the rest of that frame, as within a frame no item turns evictable, so no insert
    /// walks the slab again.
    dry_frame: Option<u64>,
    /// [`Self::evict_shelf`]'s tally, every page that holds the item one after another, kept so a
    /// starving insert allocates nothing.
    shelf_uses: Vec<ShelfUse>,
    /// The last item [`Self::evict_shelf`] found no shelf for. Cleared wherever a shelf can turn
    /// evictable: an item freed, which can empty a page's bottom shelf or a kept item's shelf; a
    /// move; a page grown or opened.
    shelf_dry: Option<ShelfDry>,
    scratch: ScratchCache,
    /// Where the age-out resumes.
    age_hand: u32,
    /// The display frame since which the list's one page has had a smaller page to shrink to, or
    /// held no item, `None` while it has neither or the list has other pages.
    sparse_since: Option<u64>,
    draining: Option<Drain>,
    /// The packer a shrink tries its items in, kept so a trial allocates nothing once warm.
    trial: ShelfPacker,
    /// The drains' moves the GPU half has yet to take ([`Self::forget_moves`]). No drain is
    /// planned while one waits, so no page a move lands on drains, and closes, before its texture
    /// took the move.
    moves: Vec<Move>,
}

/// What a drain of a page counts: the texels its items fill and the free texels of the others.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DrainCount {
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
            config.page_edge <= ShelfPacker::MAX_EDGE,
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
            shelf_dry: None,
            scratch: ScratchCache::new(format, device_edge),
            age_hand: 0,
            sparse_since: None,
            draining: None,
            trial: ShelfPacker::new(0),
            moves: Vec::new(),
        }
    }

    /// The slots a draw of the list's items reads: its open pages', slot 0 where none is open.
    pub(crate) fn slots(&self) -> PageSlots {
        let bits = self
            .pages
            .iter()
            .enumerate()
            .filter(|(_, page)| page.is_some())
            .fold(0, |bits, (idx, _)| bits | (1 << idx));
        if bits == 0 {
            PageSlots::FIRST
        } else {
            PageSlots::new(bits)
        }
    }

    /// The page at `idx`, `None` where none is open.
    pub(crate) const fn page(&self, idx: usize) -> Option<&Page> {
        self.pages[idx].as_ref()
    }

    /// Every open page with its index.
    fn open_pages(&self) -> impl Iterator<Item = (usize, &Page)> {
        self.pages
            .iter()
            .enumerate()
            .filter_map(|(idx, page)| page.as_ref().map(|page| (idx, page)))
    }

    /// The index of the list's one open page, `None` where it has none or several.
    fn only_page(&self) -> Option<usize> {
        let mut open = self.open_pages().map(|(idx, _)| idx);
        let first = open.next()?;
        open.next().is_none().then_some(first)
    }

    /// The bytes of the list's pages and scratch textures.
    pub(crate) fn bytes(&self) -> u64 {
        let pages: u64 = self.open_pages().map(|(_, page)| page.texels()).sum();
        (pages + self.scratch.texels()) * u64::from(self.format.texel_bytes())
    }

    /// The bytes the items on the list's pages fill.
    fn live_page_bytes(&self) -> u64 {
        let texels: u64 = self.open_pages().map(|(_, page)| page.live_texels).sum();
        texels * u64::from(self.format.texel_bytes())
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
        size: UVec2,
        frame: u64,
        evictions: &mut Evictions,
    ) -> Result<Allocation, AtlasError> {
        let edge = self.config.page_edge;
        if size.max_element() > edge {
            return Err(AtlasError::Oversized { size, edge });
        }
        assert!(size.x > 0 && size.y > 0, "an item holds a texel");
        // The page edge holds to what a packer counts (`Self::new`).
        let size = size.as_u16vec2();
        let placement = loop {
            if let Some(placement) = self.place(size) {
                break placement;
            }
            // The drain's source takes the item rather than the list grow beside it.
            if self.cancel_drain() {
                continue;
            }
            if self.grow(Budget::Soft) {
                continue;
            }
            if self.holds_now(size) && self.evict_for(size, frame, evictions) > 0 {
                continue;
            }
            if self.grow(Budget::Hard) {
                continue;
            }
            return Err(AtlasError::Full);
        };
        Ok(Allocation {
            handle: self.store(tenant, placement, frame),
            placement,
        })
    }

    /// The rectangle of the first page with room for `size` that is not draining.
    fn place(&mut self, size: U16Vec2) -> Option<Placement> {
        let draining = self.draining.map(|drain| usize::from(drain.page));
        for (idx, page) in self.pages.iter_mut().enumerate() {
            let Some(page) = page.as_mut().filter(|_| draining != Some(idx)) else {
                continue;
            };
            if let Some(placement) = page.allocate(idx, size) {
                return Some(placement);
            }
        }
        None
    }

    /// Whether a page holds `size` at its edge now, so an eviction can make room for it.
    fn holds_now(&self, size: U16Vec2) -> bool {
        self.open_pages().any(|(_, page)| page.holds(size))
    }

    /// Grow within `budget`, with no page draining: double the first page below the page edge,
    /// else open a page, at the first edge where the list has none and the page edge after; `false`
    /// where the budget or the slots refuse.
    fn grow(&mut self, budget: Budget) -> bool {
        debug_assert!(self.draining.is_none(), "an insert cancels a drain first");
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
            .find(|page| page.edge() < page_edge)
        {
            let edge = page.edge();
            let added = (u64::from(edge * 2).pow(2) - u64::from(edge).pow(2)) * texel;
            if bytes + added > limit {
                return false;
            }
            page.packer.grow(edge_u16(edge * 2));
            self.shelf_dry = None;
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
        if bytes + u64::from(edge).pow(2) * texel > limit {
            return false;
        }
        self.open_page(slot, edge);
        true
    }

    /// Open a page of `edge` in `slot`, which holds none.
    fn open_page(&mut self, slot: usize, edge: u32) {
        debug_assert!(self.pages[slot].is_none(), "a page opens in a free slot");
        self.pages[slot] = Some(Page::new(edge, self.opened));
        self.opened = self.opened.checked_add(1).expect("a list opened 2³² pages");
        self.shelf_dry = None;
    }

    /// Evict toward room for `size` at display frame `frame`, with no page draining: an evictable
    /// item on a shelf that takes it, chosen by the clock; with none, every item on one shelf, so
    /// its rows serve another height. How many went, `0` where none could.
    fn evict_for(&mut self, size: U16Vec2, frame: u64, evictions: &mut Evictions) -> u32 {
        debug_assert!(self.draining.is_none(), "an insert cancels a drain first");
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
            if !entry.is_idle(frame, Entry::GRACE_FRAMES) {
                return Verdict::Skip;
            }
            if page.holds(size) && page.packer.shelf(placement.origin.y).takes(size.y) {
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
    /// evictable, on a page that holds `size`: the shortest tall enough, for the item to reuse,
    /// else a page's bottom shelf, whose rows go back to its page. `0` where no shelf qualifies, as
    /// emptying a short one in the middle frees nothing usable; the latch then spares the frame's
    /// later inserts at least as large the walk.
    fn evict_shelf(&mut self, size: U16Vec2, frame: u64, evictions: &mut Evictions) -> u32 {
        if self.shelf_dry.is_some_and(|dry| dry.covers(size, frame)) {
            return 0;
        }
        // Each page that holds `size` tallies its shelves from its start in `shelf_uses`.
        let mut starts = [None; MAX_PAGES];
        let uses = &mut self.shelf_uses;
        uses.clear();
        for (idx, page) in self.pages.iter().enumerate() {
            if let Some(page) = page.as_ref().filter(|page| page.holds(size)) {
                starts[idx] = Some(uses.len());
                uses.resize(uses.len() + page.packer.shelves().len(), ShelfUse::Empty);
            }
        }
        for entry in &self.entries {
            let Some(placement) = entry.placement else {
                continue;
            };
            let page_idx = usize::from(placement.page);
            let Some(start) = starts[page_idx] else {
                continue;
            };
            let page = self.pages[page_idx]
                .as_ref()
                .expect("an item sits on an open page");
            let shelf_use = &mut uses[start + page.packer.shelf_at(placement.origin.y)];
            if !entry.is_idle(frame, Entry::GRACE_FRAMES) {
                *shelf_use = ShelfUse::Kept;
            } else if *shelf_use == ShelfUse::Empty {
                *shelf_use = ShelfUse::Evictable;
            }
        }
        let mut tall: Option<(u16, usize, u16)> = None;
        let mut bottom: Option<(usize, u16)> = None;
        for (page_idx, start) in starts.into_iter().enumerate() {
            let Some(start) = start else {
                continue;
            };
            let shelves = self.pages[page_idx]
                .as_ref()
                .expect("a tallied page is open")
                .packer
                .shelves();
            let uses = &uses[start..start + shelves.len()];
            for (shelf, &shelf_use) in shelves.iter().zip(uses) {
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
            self.shelf_dry = Some(ShelfDry { frame, size });
            return 0;
        };
        let mut evicted = 0;
        for idx in 0..self.entries.len() {
            let on_shelf = self.entries[idx]
                .placement
                .is_some_and(|p| usize::from(p.page) == page_idx && p.origin.y == shelf_y);
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
        self.pages[placement.page as usize]
            .as_mut()
            .expect("an item sits on an open page")
            .deallocate(placement);
        self.shelf_dry = None;
        self.free
            .push(u32::try_from(idx).expect("a slab counts its slots in u32"));
    }

    /// The slot `handle` names, where it still holds the item: its generation alone tells, as a
    /// slot freed moves it and a slot filled keeps it.
    fn live_idx(&self, handle: AtlasHandle) -> Option<usize> {
        debug_assert_eq!(handle.format, self.format, "a handle of another list");
        let idx = handle.idx as usize;
        let entry = self
            .entries
            .get(idx)
            .filter(|entry| entry.generation == handle.generation)?;
        debug_assert!(entry.placement.is_some(), "a live slot holds its item");
        Some(idx)
    }

    /// The entry of [`Self::live_idx`].
    fn live(&self, handle: AtlasHandle) -> Option<&Entry> {
        self.live_idx(handle).map(|idx| &self.entries[idx])
    }

    /// The entry of [`Self::live_idx`], to change.
    fn live_mut(&mut self, handle: AtlasHandle) -> Option<&mut Entry> {
        let idx = self.live_idx(handle)?;
        Some(&mut self.entries[idx])
    }

    /// Where `handle`'s item sits, `None` once it went.
    pub(crate) fn placement(&self, handle: AtlasHandle) -> Option<Placement> {
        self.live(handle)?.placement
    }

    /// Mark `handle`'s item drawn at display frame `frame`, so no eviction takes it this frame or
    /// the next, and say where it sits; `None` once it went.
    pub(crate) fn stamp(&mut self, handle: AtlasHandle, frame: u64) -> Option<Placement> {
        let entry = self.live_mut(handle)?;
        debug_assert!(entry.last_use <= frame, "the display frames ran backwards");
        entry.last_use = frame;
        entry.placement
    }

    /// Give `handle`'s item back; nothing where it went already, as an eviction may take an item
    /// any frame before its tenant drains its queue, and the generation keeps a stale handle from
    /// freeing another item.
    ///
    /// # Panics
    ///
    /// If a pin holds the item.
    pub(crate) fn release(&mut self, handle: AtlasHandle) {
        let Some(entry) = self.live(handle) else {
            return;
        };
        assert_eq!(entry.pins, 0, "a pinned item released");
        self.free_slot(handle.idx as usize);
    }

    /// Keep `handle`'s item from every eviction until a matching [`Self::unpin`], and say where it
    /// sits; `None`, and no pin, once it went.
    pub(crate) fn pin(&mut self, handle: AtlasHandle) -> Option<Placement> {
        let entry = self.live_mut(handle)?;
        entry.pins += 1;
        entry.placement
    }

    /// Take back one pin of `handle`'s item, which counts as drawn at display frame `frame`, so the
    /// clock takes the items let go longest ago first. As a stamp, it leaves the latches: the item
    /// is not evictable within the grace, so nothing turns evictable within a frame.
    ///
    /// # Panics
    ///
    /// If the item went or holds no pin: a pinned item cannot go.
    pub(crate) fn unpin(&mut self, handle: AtlasHandle, frame: u64) {
        let entry = self.live_mut(handle).expect("an unpinned item is held");
        assert!(entry.pins > 0, "an item unpinned more than pinned");
        entry.pins -= 1;
        debug_assert!(entry.last_use <= frame, "the display frames ran backwards");
        entry.last_use = frame;
    }

    /// Close page `idx`, which holds no item and is not draining, so its slot takes the next page
    /// opened.
    ///
    /// # Panics
    ///
    /// If no page is open at `idx`, or an item sits on it.
    pub(crate) fn close_page(&mut self, idx: usize) {
        debug_assert!(
            self.draining
                .is_none_or(|drain| usize::from(drain.page) != idx),
            "a drain ends before its page closes"
        );
        let page = self.pages[idx].take().expect("an open page");
        assert_eq!(page.live_texels, 0, "a page closed under its items");
    }

    /// The drains' moves the GPU half has yet to take, in the order they were made.
    pub(crate) fn moves(&self) -> &[Move] {
        &self.moves
    }

    /// Forget the moves, which the GPU half recorded.
    pub(crate) fn forget_moves(&mut self) {
        self.moves.clear();
    }

    /// The textures of their own the list hands items too large for a page.
    pub(crate) const fn scratch(&self) -> &ScratchCache {
        &self.scratch
    }

    /// [`Self::scratch`], to acquire from and release to.
    pub(crate) const fn scratch_mut(&mut self) -> &mut ScratchCache {
        &mut self.scratch
    }

    /// Return what the list holds past its need, once a display frame, at display frame `frame`
    /// (`src/gpu/AGENTS.md`, Atlases): drop the scratch textures idle an expiry window; age out
    /// what idled past the age the list's pressure sets into `evictions`; with one page that had a
    /// smaller one to shrink to, or none to close to, for an expiry window, shrink or close it;
    /// with several, drain the emptiest whose items fit the others'; move a drain's items within
    /// `budget`, for the GPU half to take ([`Self::moves`]); and close the page it emptied. No
    /// drain is planned while moves wait for the commit, so no page a move lands on drains before
    /// its texture takes the move.
    pub(crate) fn decay(&mut self, frame: u64, budget: &mut MoveBudget, evictions: &mut Evictions) {
        self.scratch.expire(frame);
        self.age_out(frame, evictions);
        if self.draining.is_none() && self.moves.is_empty() {
            match self.only_page().map(|idx| (idx, self.shrink_target(idx))) {
                Some((idx, Some(target))) => {
                    let since = *self.sparse_since.get_or_insert(frame);
                    if frame - since + 1 >= EXPIRY_FRAMES {
                        self.sparse_since = None;
                        self.shrink(idx, target);
                    }
                }
                Some((_, None)) => self.sparse_since = None,
                None => {
                    self.sparse_since = None;
                    self.plan_merge();
                }
            }
        }
        self.drain(budget);
    }

    /// Give back all the list can at once, as the system runs short of memory, at display frame
    /// `frame`: evict every item no one pins and this frame did not draw into `evictions`, drop the
    /// idle scratch and take the held scratch this frame did not draw, and drain with no budget, a
    /// sparse page shrinking and an empty one closing at once, until a drain moved an item or none
    /// is left to start. A stutter traded for the process not being killed (`src/gpu/AGENTS.md`,
    /// Atlases). Whether moves wait, after which the caller commits them and purges again, as a
    /// drain planned now could take the page they land on.
    pub(crate) fn purge(&mut self, frame: u64, evictions: &mut Evictions) -> bool {
        for idx in 0..self.entries.len() {
            let entry = &self.entries[idx];
            if entry.placement.is_some() && entry.is_idle(frame, 0) {
                self.evict(idx, evictions);
            }
        }
        self.scratch.purge(frame, evictions);
        let mut budget = MoveBudget {
            texels: u64::MAX,
            moves: u32::MAX,
        };
        // A drain ends with its page closed or stalled, and a shrink opens one page and closes
        // another, so the rounds are at most the pages and one more. A drain under way goes on,
        // as its moves all leave its page; a new one waits for the moves before it.
        for _ in 0..=MAX_PAGES {
            if self.draining.is_none() {
                if !self.moves.is_empty() {
                    break;
                }
                self.sparse_since = None;
                match self.only_page() {
                    Some(idx) => {
                        if let Some(target) = self.shrink_target(idx) {
                            self.shrink(idx, target);
                        }
                    }
                    None => self.plan_merge(),
                }
            }
            if self.draining.is_none() {
                break;
            }
            self.drain(&mut budget);
        }
        !self.moves.is_empty()
    }

    /// Evict the items no one pins and no frame drew for WebRender's age at the list's pressure,
    /// at most [`AGE_EVICTIONS_MAX`] below four times the soft budget, walking at most
    /// [`AGE_SCAN_MAX`] slots. The pressure is the bytes the pages' items fill over the soft
    /// budget, as WebRender's `bytes_allocated` counts its entries, not its textures; and nothing
    /// ages below the budget on one page, grown in place or not, as WebRender's
    /// `should_continue_evicting` ages below its threshold only across several textures. Refused:
    /// the pages' bytes as the pressure and a grown page aged under the budget, which evicted each
    /// glyph of a zoom over 512 scales just before its scale came back, though the set fit
    /// (`.notes/research/2026-11-12-atlas-pool-measurements.md`). Cost: a burst that grew the one
    /// page keeps its items, at most a page at the page edge, until they fill the budget or a
    /// second page opens.
    fn age_out(&mut self, frame: u64, evictions: &mut Evictions) {
        let live = self.live_page_bytes();
        let soft = self.config.soft_bytes;
        if live < soft && self.open_pages().nth(1).is_none() {
            return;
        }
        let ratio = live.checked_div(soft).unwrap_or(u64::MAX);
        let age = age_threshold(ratio);
        let evictions_max = if soft == 0 || live > 4 * soft {
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
            // Every age is at least the grace: the last frame is `frame - 1`.
            if entry.placement.is_some() && entry.is_idle(frame, age) {
                self.evict(idx, evictions);
                evicted += 1;
            }
        }
    }

    /// What page `idx`, the list's one, would shrink to: none where it holds no item, so it
    /// closes; else a page at the smallest power-of-two edge from the first that holds twice its
    /// items' texels, so an eighth full at most, landing at half or under as a `Vec` halves. `None`
    /// where that is no smaller, or the page is stalled.
    fn shrink_target(&self, idx: usize) -> Option<Shrink> {
        let page = self.pages[idx].as_ref().expect("the one page is open");
        if page.live_texels == 0 {
            return Some(Shrink::Close);
        }
        let mut edge = self.config.first_edge;
        while u64::from(edge).pow(2) < 2 * page.live_texels {
            edge *= 2;
        }
        (edge < page.edge() && !self.is_stalled(idx)).then_some(Shrink::To(edge))
    }

    /// Shrink page `idx`, the list's one, as `target`, its [`Self::shrink_target`], says, with no
    /// move waiting: close it where it holds no item, else open the smallest page from the target
    /// that packs its items beside it and drain into that. Where none smaller packs them, stall the
    /// page until they change.
    fn shrink(&mut self, idx: usize, target: Shrink) {
        let mut edge = match target {
            Shrink::Close => {
                self.close_page(idx);
                return;
            }
            Shrink::To(edge) => edge,
        };
        let page_edge = self.pages[idx]
            .as_ref()
            .expect("the one page is open")
            .edge();
        // Once a window, not a frame. The texels alone would open a page a wide item or a mix of
        // heights does not pack into, whose drain stalls and drains back, each window again.
        while !self.packs_into(edge) {
            edge *= 2;
            if edge >= page_edge {
                self.stall(idx);
                return;
            }
        }
        let slot = self
            .pages
            .iter()
            .position(Option::is_none)
            .expect("a list of one page has free slots");
        self.open_page(slot, edge);
        self.begin_drain(idx);
    }

    /// With several pages and no move waiting, drain the emptiest whose items fit half the others'
    /// free texels, so packing slack leaves room, passing a stalled page; none where no page
    /// qualifies. Refused: the emptiest alone, which a stall held: three pages kept where two
    /// would do.
    fn plan_merge(&mut self) {
        let mut order = [None; MAX_PAGES];
        for (slot, (idx, page)) in order.iter_mut().zip(self.open_pages()) {
            *slot = Some((page.live_texels, idx));
        }
        // The empty slots sort first, as `None` does, and drop out of the walk.
        order.sort_unstable();
        for (_, idx) in order.into_iter().flatten() {
            let count = self.drain_count(idx);
            if !self.is_stalled(idx) && count.live_texels * 2 <= count.others_free {
                self.begin_drain(idx);
                return;
            }
        }
    }

    /// Whether a fresh page of `edge` takes every item of the list, placed in the slab's order as
    /// a drain into it places them.
    fn packs_into(&mut self, edge: u32) -> bool {
        let Self { entries, trial, .. } = self;
        trial.reset(edge_u16(edge));
        entries
            .iter()
            .filter_map(|entry| entry.placement)
            .all(|placement| trial.allocate(placement.size).is_some())
    }

    /// Whether page `idx`, which is open, stalled at a drain or a shrink and nothing it counts
    /// changed since.
    fn is_stalled(&self, idx: usize) -> bool {
        let page = self.pages[idx].as_ref().expect("an open page");
        page.stalled == Some(self.drain_count(idx))
    }

    /// Mark page `idx`, which is open, stalled at what a drain of it counts now.
    fn stall(&mut self, idx: usize) {
        let count = self.drain_count(idx);
        self.pages[idx].as_mut().expect("an open page").stalled = Some(count);
    }

    /// What a drain of page `idx`, which is open, counts.
    fn drain_count(&self, idx: usize) -> DrainCount {
        let free = |page: &Page| page.texels() - page.live_texels;
        let page = self.pages[idx].as_ref().expect("an open page");
        DrainCount {
            live_texels: page.live_texels,
            others_free: self.open_pages().map(|(_, page)| free(page)).sum::<u64>() - free(page),
        }
    }

    /// Begin the drain of page `idx`, which is open.
    fn begin_drain(&mut self, idx: usize) {
        debug_assert!(self.draining.is_none(), "one page drains at a time");
        self.draining = Some(Drain {
            page: u8::try_from(idx).expect("a list holds few pages"),
            hand: 0,
        });
    }

    /// Move the draining page's items onto the others within `budget`, each placed as an insert
    /// would, and close the page once it holds none. A move that finds no room cancels the drain
    /// and stalls the page until what it counts changes, spending none of the budget.
    fn drain(&mut self, budget: &mut MoveBudget) {
        let Some(Drain { page, hand }) = self.draining else {
            return;
        };
        let source = usize::from(page);
        let from_epoch = self.pages[source]
            .as_ref()
            .expect("a draining page is open")
            .epoch;
        let mut resume = self.entries.len();
        for at in hand as usize..self.entries.len() {
            let Some(from) = self.entries[at]
                .placement
                .filter(|placement| placement.page == page)
            else {
                continue;
            };
            if budget.is_spent() {
                resume = at;
                break;
            }
            let Some(to) = self.place(from.size) else {
                self.draining = None;
                self.stall(source);
                return;
            };
            budget.spend(from.texels());
            self.pages[source]
                .as_mut()
                .expect("the source is open")
                .deallocate(from);
            self.shelf_dry = None;
            self.entries[at].placement = Some(to);
            self.moves.push(Move {
                from,
                from_epoch,
                to,
            });
        }
        let live = self.pages[source]
            .as_ref()
            .expect("the source is open")
            .live_texels;
        if live == 0 {
            self.draining = None;
            self.close_page(source);
        } else {
            debug_assert!(resume < self.entries.len(), "a whole walk empties the page");
            self.draining = Some(Drain {
                page,
                hand: u32::try_from(resume).expect("a slab counts its slots in u32"),
            });
        }
    }

    /// End a drain with its page kept, which takes allocations again, `true` where one was under
    /// way.
    const fn cancel_drain(&mut self) -> bool {
        self.draining.take().is_some()
    }
}

/// An edge as a packer counts it, which [`PageList::new`] holds to [`ShelfPacker::MAX_EDGE`].
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
    use crate::gpu::atlas::page_list::PageListConfig;

    impl Page {
        /// The texels its items fill.
        pub(crate) const fn live_texels(&self) -> u64 {
            self.live_texels
        }
    }

    #[cfg(test)]
    impl PageList {
        pub(crate) const fn config(&self) -> PageListConfig {
            self.config
        }

        /// The page a drain empties, `None` where none is under way.
        pub(crate) fn draining(&self) -> Option<usize> {
            self.draining.map(|drain| usize::from(drain.page))
        }

        /// The display frame `handle`'s item was last drawn in, `None` once it went.
        pub(crate) fn last_use(&self, handle: AtlasHandle) -> Option<u64> {
            self.live(handle).map(|entry| entry.last_use)
        }

        /// The pins on `handle`'s item, `None` once it went.
        pub(crate) fn pins(&self, handle: AtlasHandle) -> Option<u32> {
            self.live(handle).map(|entry| entry.pins)
        }
    }
}

#[cfg(test)]
mod tests;
