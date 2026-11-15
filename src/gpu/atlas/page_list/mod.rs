//! [`PageList`]: the pages of one texel format, the items packed on them, and the textures of
//! their own it hands items too large for a page.

use crate::common::clock_sweep::{self, AGE_EVICTIONS_MAX, AGE_SCAN_MAX, ClockSweep, Verdict};
use crate::common::expiry_window::EXPIRY_FRAMES;
use crate::common::gen_slab::{GenSlab, SlabKey};
use crate::gpu::atlas::Evictions;
use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::atlas::atlas_handle::{Allocation, AtlasHandle, Evicted, Placement};
use crate::gpu::atlas::error::AtlasError;
use crate::gpu::atlas::page_list::move_budget::MoveBudget;
use crate::gpu::atlas::page_list::page::Page;
use crate::gpu::atlas::page_slots::PageSlots;
use crate::gpu::atlas::scratch_cache::ScratchCache;
use crate::gpu::atlas::tenant::ItemOwner;
use crate::gpu::resource::shelf_packer::ShelfPacker;
use glam::{U16Vec2, UVec2};

pub(crate) mod move_budget;
pub(crate) mod page;

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

/// An item of the slab: where it sits, the display frame it was last drawn in, the pins on it, and
/// who holds it.
#[derive(Clone, Copy, Debug)]
struct Entry {
    placement: Placement,
    last_use: u64,
    pins: u32,
    owner: ItemOwner,
}

// A slab slot holds an entry in 32 bytes, its tag included, as the generations are a column of
// their own.
const _: () = assert!(size_of::<Option<Entry>>() == 32);

impl Entry {
    /// Whether no one pins it and no frame drew it in the `age` frames before display frame
    /// `frame`, nor in `frame`: what an eviction may take, the clock at an age of
    /// [`GRACE_FRAMES`](clock_sweep::GRACE_FRAMES), the age-out at its pressure's, a purge at
    /// none.
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

/// A shelf [`PageList::evict_shelf`] may empty: its page and its top row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ShelfAt {
    page: usize,
    y: u16,
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
    entries: GenSlab<Entry>,
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
            entries: GenSlab::new(),
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

    /// Room for `owner`'s item of `size` at display frame `frame`, evicting into `evictions` as the
    /// insert needs.
    pub(crate) fn allocate(
        &mut self,
        owner: ItemOwner,
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
        let key = self.entries.insert(Entry {
            placement,
            last_use: frame,
            pins: 0,
            owner,
        });
        Ok(Allocation {
            handle: self.handle(key),
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
        let sweep = ClockSweep::over(entries.slot_count(), self.hand, |idx| {
            let Some(entry) = entries.at(idx) else {
                return Verdict::Skip;
            };
            let placement = entry.placement;
            let page = pages[placement.page as usize]
                .as_ref()
                .expect("an item sits on an open page");
            if !entry.is_idle(frame, clock_sweep::GRACE_FRAMES) {
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
        for entry in self.entries.values() {
            let placement = entry.placement;
            let page_idx = usize::from(placement.page);
            let Some(start) = starts[page_idx] else {
                continue;
            };
            let page = self.pages[page_idx]
                .as_ref()
                .expect("an item sits on an open page");
            let shelf_use = &mut uses[start + page.packer.shelf_at(placement.origin.y)];
            if !entry.is_idle(frame, clock_sweep::GRACE_FRAMES) {
                *shelf_use = ShelfUse::Kept;
            } else if *shelf_use == ShelfUse::Empty {
                *shelf_use = ShelfUse::Evictable;
            }
        }
        // The shortest evictable shelf tall enough, with its height, and the first page's
        // evictable bottom shelf.
        let mut tall: Option<(u16, ShelfAt)> = None;
        let mut bottom: Option<ShelfAt> = None;
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
                    && tall.is_none_or(|(height, _)| shelf.height < height)
                {
                    tall = Some((
                        shelf.height,
                        ShelfAt {
                            page: page_idx,
                            y: shelf.y,
                        },
                    ));
                }
            }
            if bottom.is_none() && uses.last() == Some(&ShelfUse::Evictable) {
                let last = shelves.last().expect("a tally a shelf");
                bottom = Some(ShelfAt {
                    page: page_idx,
                    y: last.y,
                });
            }
        }
        let Some(ShelfAt {
            page: page_idx,
            y: shelf_y,
        }) = tall.map(|(_, at)| at).or(bottom)
        else {
            self.shelf_dry = Some(ShelfDry { frame, size });
            return 0;
        };
        let mut evicted = 0;
        for idx in 0..self.entries.slot_count() {
            let on_shelf = self.entries.at(idx).is_some_and(|entry| {
                usize::from(entry.placement.page) == page_idx && entry.placement.origin.y == shelf_y
            });
            if on_shelf {
                self.evict(idx, evictions);
                evicted += 1;
            }
        }
        evicted
    }

    /// Take the item at slab slot `idx` and queue it for its tenant.
    fn evict(&mut self, idx: usize, evictions: &mut Evictions) {
        let key = self
            .entries
            .key_at(idx)
            .expect("an evicted slot holds an item");
        let entry = self.free(key);
        evictions.push(
            entry.owner.tenant,
            Evicted {
                handle: self.handle(key),
                tag: entry.owner.tag,
            },
        );
    }

    /// The handle of the item `key` names.
    const fn handle(&self, key: SlabKey) -> AtlasHandle {
        AtlasHandle {
            format: self.format,
            key,
        }
    }

    /// Give the item `key` names back: its rectangle to its page, its slot to the slab, whose key
    /// every handle to it reads as stale now.
    fn free(&mut self, key: SlabKey) -> Entry {
        let entry = self.entries.remove(key).expect("a held item frees once");
        self.pages[entry.placement.page as usize]
            .as_mut()
            .expect("an item sits on an open page")
            .deallocate(entry.placement);
        self.shelf_dry = None;
        entry
    }

    /// The entry `handle` names, `None` once it went.
    fn live(&self, handle: AtlasHandle) -> Option<&Entry> {
        debug_assert_eq!(handle.format, self.format, "a handle of another list");
        self.entries.get(handle.key)
    }

    /// The entry `handle` names, to change, `None` once it went.
    fn live_mut(&mut self, handle: AtlasHandle) -> Option<&mut Entry> {
        debug_assert_eq!(handle.format, self.format, "a handle of another list");
        self.entries.get_mut(handle.key)
    }

    /// Where `handle`'s item sits, `None` once it went.
    pub(crate) fn placement(&self, handle: AtlasHandle) -> Option<Placement> {
        Some(self.live(handle)?.placement)
    }

    /// Mark `handle`'s item drawn at display frame `frame`, so no eviction takes it this frame or
    /// the next, and say where it sits; `None` once it went.
    pub(crate) fn stamp(&mut self, handle: AtlasHandle, frame: u64) -> Option<Placement> {
        let entry = self.live_mut(handle)?;
        debug_assert!(entry.last_use <= frame, "the display frames ran backwards");
        entry.last_use = frame;
        Some(entry.placement)
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
        self.free(handle.key);
    }

    /// Keep `handle`'s item from every eviction until a matching [`Self::unpin`], and say where it
    /// sits; `None`, and no pin, once it went.
    pub(crate) fn pin(&mut self, handle: AtlasHandle) -> Option<Placement> {
        let entry = self.live_mut(handle)?;
        entry.pins += 1;
        Some(entry.placement)
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
        for idx in 0..self.entries.slot_count() {
            if self
                .entries
                .at(idx)
                .is_some_and(|entry| entry.is_idle(frame, 0))
            {
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
        let len = self.entries.slot_count();
        let mut evicted = 0;
        for _ in 0..len.min(AGE_SCAN_MAX) {
            if evicted == evictions_max {
                break;
            }
            let idx = self.age_hand as usize % len;
            self.age_hand = u32::try_from((idx + 1) % len).expect("a slab counts its slots in u32");
            // Every age is at least the grace: the last frame is `frame - 1`.
            if self
                .entries
                .at(idx)
                .is_some_and(|entry| entry.is_idle(frame, age))
            {
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
            .values()
            .all(|entry| trial.allocate(entry.placement.size).is_some())
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
        let mut resume = self.entries.slot_count();
        for at in hand as usize..self.entries.slot_count() {
            let Some(from) = self
                .entries
                .at(at)
                .map(|entry| entry.placement)
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
            self.entries
                .at_mut(at)
                .expect("the moved item is held")
                .placement = to;
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
            debug_assert!(
                resume < self.entries.slot_count(),
                "a whole walk empties the page"
            );
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
    #[cfg(test)]
    use crate::gpu::atlas::page_list::PageList;
    #[cfg(test)]
    use crate::gpu::atlas::page_list::PageListConfig;

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
