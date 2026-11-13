//! The list's inserts: placements, growth within each budget, the clock's victims and exemptions,
//! the shelf it empties, page states and the device's edge; and its decay ([`decay`]).

mod decay;

use super::*;

/// An `R8` list of `first` and `page` edges and `soft` bytes on a device of 16384.
fn list(first: u32, page: u32, soft: u64) -> PageList {
    PageList::new(
        AtlasFormat::R8,
        PageListConfig {
            first_edge: first,
            page_edge: page,
            soft_bytes: soft,
        },
        16384,
    )
}

fn square(side: u32) -> UVec2 {
    UVec2::splat(side)
}

/// The placement of `handle`, which is live.
fn at(list: &PageList, handle: AtlasHandle) -> (u8, U16Vec2) {
    let placement = list.placement(handle).expect("a live item");
    (placement.page, placement.origin)
}

/// The first page opens at 64 and packs 32² items on 32-texel shelves: (0, 0), (32, 0), (0, 32),
/// (32, 32). A fifth doubles it to the 128 page edge, keeping every origin, and lands on the
/// first shelf's new columns at (64, 0). A 100² item takes no 32-texel shelf, and the 128 page's
/// 64 unclaimed rows cannot hold it, so page 1 opens at the page edge and takes it at (0, 0).
/// The list holds `2 · 128² = 32768` bytes, its items `5 · 32² = 5120` texels on page 0 and
/// `100² = 10000` on page 1. An item past the page edge is refused. Its open slots read 0 while it
/// holds none and one page, and 0 and 1 once the second opens.
#[test]
fn a_list_packs_doubles_its_first_page_then_opens_pages() {
    let mut list = list(64, 128, 1 << 20);
    assert_eq!(list.slots(), PageSlots::FIRST, "no page open");
    let mut evictions = Evictions::default();
    let mut alloc = |list: &mut PageList, size| {
        list.allocate(Tenant::Glyphs, size, 0, &mut evictions)
            .unwrap()
            .handle
    };
    let items: Vec<AtlasHandle> = (0..5).map(|_| alloc(&mut list, square(32))).collect();
    let origins: Vec<(u8, U16Vec2)> = items.iter().map(|&item| at(&list, item)).collect();
    assert_eq!(
        origins,
        [
            (0, U16Vec2::new(0, 0)),
            (0, U16Vec2::new(32, 0)),
            (0, U16Vec2::new(0, 32)),
            (0, U16Vec2::new(32, 32)),
            (0, U16Vec2::new(64, 0)),
        ]
    );
    assert_eq!(list.page(0).unwrap().edge(), 128);
    assert_eq!(list.slots(), PageSlots::FIRST);
    let big = alloc(&mut list, square(100));
    assert_eq!(at(&list, big), (1, U16Vec2::ZERO));
    assert_eq!(list.slots(), PageSlots::new(0b11));
    assert_eq!(list.page(1).unwrap().edge(), 128);
    assert_eq!(list.bytes(), 2 * 128 * 128);
    assert_eq!(
        [0, 1].map(|page| list.page(page).unwrap().live_texels()),
        [5 * 32 * 32, 100 * 100]
    );
    assert_eq!(
        list.allocate(Tenant::Glyphs, UVec2::new(129, 1), 0, &mut evictions),
        Err(AtlasError::Oversized {
            size: UVec2::new(129, 1),
            edge: 128
        })
    );
}

/// Over the soft budget of two 64 pages, `8192` bytes, the list evicts before it grows, and takes
/// neither an item drawn this frame or the last nor a pinned one: at frame 2, `a`, stamped at 1, and
/// pinned `b` stay, and the clock takes `c` for `i` and `d`, an icon, for `j`, each new item in its
/// victim's place, each victim queued for its own tenant and stale. With every item kept, the
/// list grows past the budget, to a third page, rather than refuse.
#[test]
fn over_budget_a_list_evicts_what_no_frame_holds() {
    let mut list = list(64, 64, 2 * 64 * 64);
    let mut evictions = Evictions::default();
    let tenants = [
        Tenant::Glyphs,
        Tenant::Glyphs,
        Tenant::Glyphs,
        Tenant::Icons,
    ];
    let items: Vec<AtlasHandle> = (0..8)
        .map(|i| {
            list.allocate(
                *tenants.get(i).unwrap_or(&Tenant::Glyphs),
                square(32),
                0,
                &mut evictions,
            )
            .unwrap()
            .handle
        })
        .collect();
    assert_eq!(list.bytes(), 2 * 64 * 64, "two pages, at the budget");
    let [a, b, c, d] = [items[0], items[1], items[2], items[3]];
    list.stamp(a, 1);
    list.pin(b);

    let i = list
        .allocate(Tenant::Glyphs, square(32), 2, &mut evictions)
        .unwrap()
        .handle;
    assert_eq!(at(&list, i), (0, U16Vec2::new(0, 32)), "c's place");
    assert_eq!(list.placement(c), None, "c is stale");
    let j = list
        .allocate(Tenant::Glyphs, square(32), 2, &mut evictions)
        .unwrap()
        .handle;
    assert_eq!(at(&list, j), (0, U16Vec2::new(32, 32)), "d's place");
    assert_eq!(evictions.queues[Tenant::Glyphs as usize], [c]);
    assert_eq!(evictions.queues[Tenant::Icons as usize], [d]);
    assert_eq!(
        list.bytes(),
        2 * 64 * 64,
        "no grow while something is evictable"
    );

    for &item in &items[4..] {
        list.stamp(item, 2);
    }
    list.stamp(i, 2);
    list.stamp(j, 2);
    let k = list
        .allocate(Tenant::Glyphs, square(32), 2, &mut evictions)
        .unwrap()
        .handle;
    assert_eq!(
        at(&list, k),
        (2, U16Vec2::ZERO),
        "a third page, past the budget"
    );
    assert_eq!(list.bytes(), 3 * 64 * 64);
    assert!(list.placement(a).is_some() && list.placement(b).is_some());
}

/// At the hard cap, four pages each full of items drawn this frame, an insert is refused; once a
/// frame passes with none drawn, it evicts.
#[test]
fn at_the_hard_cap_a_list_refuses_until_a_frame_frees_one() {
    let mut list = list(32, 32, 0);
    let mut evictions = Evictions::default();
    for _ in 0..MAX_PAGES {
        list.allocate(Tenant::Glyphs, square(32), 0, &mut evictions)
            .unwrap();
    }
    assert_eq!(
        list.allocate(Tenant::Glyphs, square(32), 1, &mut evictions),
        Err(AtlasError::Full)
    );
    let fresh = list
        .allocate(Tenant::Glyphs, square(32), 2, &mut evictions)
        .unwrap()
        .handle;
    assert_eq!(
        at(&list, fresh),
        (0, U16Vec2::ZERO),
        "the first page's, evicted"
    );
    assert_eq!(evictions.queues[Tenant::Glyphs as usize].len(), 1);
}

/// With no evictable item on a shelf that takes a 64 × 32 item, the list empties a whole shelf: on
/// a 64 page of sixteen 16² items, four to each 16-texel shelf, every one evictable, it frees the
/// bottom shelf, items 12 to 15, whose rows go back to the page; 16 rows are not 32, so it frees
/// the next, items 8 to 11, and the item lands at (0, 32), with no grow.
#[test]
fn a_tall_item_frees_bottom_shelves_until_rows_hold_it() {
    let mut list = list(64, 64, 64 * 64);
    let mut evictions = Evictions::default();
    let items: Vec<AtlasHandle> = (0..16)
        .map(|_| {
            list.allocate(Tenant::Glyphs, square(16), 0, &mut evictions)
                .unwrap()
                .handle
        })
        .collect();
    let tall = list
        .allocate(Tenant::Glyphs, UVec2::new(64, 32), 2, &mut evictions)
        .unwrap()
        .handle;
    assert_eq!(at(&list, tall), (0, U16Vec2::new(0, 32)));
    let evicted: Vec<AtlasHandle> = [12, 13, 14, 15, 8, 9, 10, 11]
        .map(|idx| items[idx])
        .to_vec();
    assert_eq!(evictions.queues[Tenant::Glyphs as usize], evicted);
    assert_eq!(list.bytes(), 64 * 64, "no grow");
}

/// A draining page takes no allocation while another page has room, and an insert that finds none
/// cancels the drain rather than grow beside it. Four 16² items fill the first 32² page, a fifth
/// opens the second at (0, 0); three of the first page's go. A decay under a budget of no move
/// begins draining the first, whose one item fits the second's free texels twice: three inserts
/// pass it for the second page's rest; a fourth finds no room there, so the first page takes
/// allocations again and the insert lands at (16, 0), where the second item was.
#[test]
fn a_draining_page_takes_no_allocation_until_an_insert_needs_it() {
    let mut list = list(32, 32, 1 << 20);
    let mut evictions = Evictions::default();
    let mut insert = |list: &mut PageList| {
        list.allocate(Tenant::Glyphs, square(16), 0, &mut evictions)
            .unwrap()
            .handle
    };
    let first: Vec<AtlasHandle> = (0..4).map(|_| insert(&mut list)).collect();
    let fifth = insert(&mut list);
    assert_eq!(at(&list, fifth), (1, U16Vec2::ZERO));
    for &handle in &first[1..] {
        list.release(handle);
    }
    let mut none = MoveBudget {
        texels: u64::MAX,
        moves: 0,
    };
    list.decay(0, &mut none, &mut Evictions::default());
    assert_eq!(list.draining(), Some(0));
    let passed: Vec<(u8, U16Vec2)> = (0..3)
        .map(|_| {
            let handle = insert(&mut list);
            at(&list, handle)
        })
        .collect();
    assert_eq!(
        passed,
        [
            (1, U16Vec2::new(16, 0)),
            (1, U16Vec2::new(0, 16)),
            (1, U16Vec2::new(16, 16))
        ]
    );
    let cancelled = insert(&mut list);
    assert_eq!(at(&list, cancelled), (0, U16Vec2::new(16, 0)));
    assert_eq!(list.draining(), None);
    assert_eq!(at(&list, first[0]), (0, U16Vec2::ZERO));
}

/// A released item's handle reads as stale and its slot takes the next insert at a new
/// generation; a pinned item may not be released, nor an item unpinned past its pins. An
/// allocation says where its item sits. A stale handle frees, pins and stamps nothing, though its
/// slot holds the next item; a pin says where a live item sits, and an unpin at frame 3 counts as
/// a draw at 3.
#[test]
fn a_released_slot_serves_the_next_insert_at_a_new_generation() {
    let mut list = list(32, 32, 1 << 20);
    let mut evictions = Evictions::default();
    let first = list
        .allocate(Tenant::Glyphs, square(8), 0, &mut evictions)
        .unwrap();
    assert_eq!(list.placement(first.handle), Some(first.placement));
    assert_eq!(first.placement.size, U16Vec2::splat(8));
    let first = first.handle;
    list.release(first);
    assert_eq!(list.placement(first), None);
    assert_eq!(list.stamp(first, 1), None);
    let second = list
        .allocate(Tenant::Icons, square(8), 1, &mut evictions)
        .unwrap()
        .handle;
    assert_eq!(
        (second.idx, second.generation),
        (first.idx, first.generation + 1)
    );
    assert_eq!(list.page(0).unwrap().live_texels(), 64);

    list.release(first);
    assert_eq!(list.pin(first), None);
    assert!(
        list.placement(second).is_some(),
        "a stale release frees nothing"
    );
    assert_eq!(list.pins(second), Some(0), "a stale pin pins nothing");
    assert_eq!(list.pin(second), list.placement(second));
    assert_eq!(list.pins(second), Some(1));
    list.unpin(second, 3);
    assert_eq!(list.pins(second), Some(0));
    assert_eq!(list.last_use(second), Some(3), "an unpin counts as a draw");
}

/// A device whose passes move their whole target moves by copy commands, 16 a frame; any other by
/// draw, with no count of moves; both stop at 512² texels.
#[test]
fn a_frame_moves_within_its_device_budget() {
    assert_eq!(
        MoveBudget::of(PassTraffic::WholeTarget),
        MoveBudget {
            texels: 512 * 512,
            moves: 16,
        }
    );
    assert_eq!(
        MoveBudget::of(PassTraffic::Drawn),
        MoveBudget {
            texels: 512 * 512,
            moves: u32::MAX,
        }
    );
}

#[test]
#[should_panic(expected = "a pinned item released")]
fn a_pinned_item_is_not_released() {
    let mut list = list(32, 32, 1 << 20);
    let item = list
        .allocate(Tenant::Glyphs, square(8), 0, &mut Evictions::default())
        .unwrap()
        .handle;
    list.pin(item);
    list.release(item);
}

#[test]
#[should_panic(expected = "unpinned more than pinned")]
fn an_item_is_not_unpinned_past_its_pins() {
    let mut list = list(32, 32, 1 << 20);
    let item = list
        .allocate(Tenant::Glyphs, square(8), 0, &mut Evictions::default())
        .unwrap()
        .handle;
    list.unpin(item, 0);
}

/// The page edge holds to the largest power of two the device's edge reaches, 1024 on a device of
/// 1500, and the first edge to the page edge, 128 on a device of 200. An item past half a page
/// prefers scratch: 512 of 1024 does not, 513 does. Scratch counts in the bytes: a 100² request
/// takes a 128² texture, 16384 bytes of `R8`.
#[test]
fn edges_hold_to_the_device_and_scratch_counts_in_the_bytes() {
    let config = PageListConfig {
        first_edge: 256,
        page_edge: 2048,
        soft_bytes: 1 << 20,
    };
    let mut wide = PageList::new(AtlasFormat::R8, config, 1500);
    assert_eq!(
        (wide.config().first_edge, wide.config().page_edge),
        (256, 1024)
    );
    let narrow = PageList::new(AtlasFormat::R8, config, 200);
    assert_eq!(
        (narrow.config().first_edge, narrow.config().page_edge),
        (128, 128)
    );
    assert!(!wide.prefers_scratch(UVec2::new(512, 10)));
    assert!(wide.prefers_scratch(UVec2::new(10, 513)));
    let id = wide
        .scratch_mut()
        .acquire(UVec2::splat(100), Tenant::Backdrops, 0)
        .unwrap();
    assert_eq!(wide.scratch().size(id), Some(UVec2::splat(128)));
    assert_eq!(wide.bytes(), 128 * 128);
}

#[test]
#[should_panic(expected = "a page closed under its items")]
fn a_page_is_not_closed_under_its_items() {
    let mut list = list(32, 32, 1 << 20);
    list.allocate(Tenant::Glyphs, square(8), 0, &mut Evictions::default())
        .unwrap();
    list.close_page(0);
}

/// An item no shelf takes and no whole shelf frees latches for the frame, and a release opens the
/// latch. Four 32² pages at the hard cap hold four 16² items each, one pinned on each of every
/// page's two shelves, the rest idle: a 32² item at frame 2 finds no victim on a shelf that takes
/// it and no shelf all evictable, and is refused, twice. Once page 0's two pinned items are
/// unpinned and released, each of its shelves holds one idle item: the third empties both and
/// lands at the page's origin.
#[test]
fn a_shelf_no_insert_can_free_latches_until_a_release() {
    let mut list = list(32, 32, 0);
    let mut evictions = Evictions::default();
    let items: Vec<AtlasHandle> = (0..16)
        .map(|_| {
            list.allocate(Tenant::Glyphs, square(16), 0, &mut evictions)
                .unwrap()
                .handle
        })
        .collect();
    for page in items.chunks(4) {
        list.pin(page[0]);
        list.pin(page[2]);
    }
    for _ in 0..2 {
        assert_eq!(
            list.allocate(Tenant::Glyphs, square(32), 2, &mut evictions),
            Err(AtlasError::Full)
        );
    }
    for handle in [items[0], items[2]] {
        list.unpin(handle, 2);
        list.release(handle);
    }
    let tall = list
        .allocate(Tenant::Glyphs, square(32), 2, &mut evictions)
        .unwrap()
        .handle;
    assert_eq!(at(&list, tall), (0, U16Vec2::ZERO));
    assert_eq!(
        evictions.queues[Tenant::Glyphs as usize],
        [items[3], items[1]]
    );
}

/// A dry shelf latched for a 16 × 8 item at frame 3 covers, that frame, an item no shorter and no
/// narrower: as large, taller, or wider. A shorter item, which more shelves take, a narrower one,
/// whose longer side more pages hold, or the next frame finds the latch open.
#[test]
fn a_dry_shelf_covers_what_is_no_shorter_and_no_narrower() {
    let dry = ShelfDry {
        frame: 3,
        size: U16Vec2::new(16, 8),
    };
    let covers = |w, h, frame| dry.covers(U16Vec2::new(w, h), frame);
    assert!(covers(16, 8, 3), "the same item");
    assert!(covers(16, 12, 3), "taller");
    assert!(covers(20, 8, 3), "wider");
    assert!(!covers(16, 6, 3), "shorter");
    assert!(!covers(12, 8, 3), "narrower: its longer side 12 below 16");
    assert!(!covers(16, 8, 4), "the next frame");
}
