//! The list's decay: the age-out at each pressure, the drain of the emptiest page, the shrink of a
//! sparse one, and the moves each makes within a frame's budget.

use super::{at, list, square};
use crate::gpu::atlas::Evictions;
use crate::gpu::atlas::atlas_handle::{AtlasHandle, Placement};
use crate::gpu::atlas::page_list::{DecayCtx, DrainCount, Move, MoveBudget, PageList, PageState};
use crate::gpu::atlas::tenant::Tenant;
use glam::U16Vec2;

/// A frame's budget past anything these lists move.
const ROOMY: MoveBudget = MoveBudget {
    texels: u64::MAX,
    copies: u32::MAX,
};

/// `n` items of `side`² for glyphs, at frame 0.
fn insert(list: &mut PageList, n: usize, side: u16) -> Vec<AtlasHandle> {
    let mut evictions = Evictions::default();
    (0..n)
        .map(|_| {
            list.allocate(Tenant::Glyphs, square(side), 0, &mut evictions)
                .unwrap()
        })
        .collect()
}

/// The handles one decay at `frame` evicted, and the moves it made, within `budget`.
#[derive(Debug)]
struct Decayed {
    evicted: Vec<AtlasHandle>,
    moves: Vec<Move>,
}

fn decay(list: &mut PageList, frame: u64, mut budget: MoveBudget) -> Decayed {
    let mut evictions = Evictions::default();
    let mut moves = Vec::new();
    list.decay(
        frame,
        DecayCtx {
            budget: &mut budget,
            evictions: &mut evictions,
            moves: &mut moves,
        },
    );
    Decayed {
        evicted: evictions.queues[Tenant::Glyphs as usize].clone(),
        moves,
    }
}

/// The age-out follows WebRender's table over the list's pressure. 40 items of 8² on 32² pages,
/// sixteen a page, take three pages, 3072 bytes; one is pinned and one drawn at frame 50.
///
/// Under a soft budget of 1024 the pressure is 3: an item goes once idle past 50 frames, 32 a
/// frame. At frame 50 none is past it; at 51 the clock takes the 32 slots after the two kept ones,
/// which empty the second page, and the drain closes it. At 52 nothing is idle past the age of
/// 100 two pages set, and the first page, now the emptiest, drains its two kept items into the
/// third and closes: one page of 1024 bytes, a pressure of 1 and an age of 200, so the third
/// page's other six go at frame 201, not at 101.
///
/// Under a budget of 512 the pressure is 6, past four times it: the age is 5 and no cap holds, so
/// all 38 go at frame 6.
#[test]
fn the_age_out_follows_the_pressure() {
    let setup = |soft: u64| {
        let mut list = list(32, 32, soft);
        let items = insert(&mut list, 40, 8);
        assert_eq!(list.bytes(), 3 * 32 * 32);
        list.pin(items[0]);
        list.stamp(items[1], 50).unwrap();
        (list, items)
    };
    let (mut list, items) = setup(1024);
    let evicted = |list: &mut PageList, frame| decay(list, frame, ROOMY).evicted.len();
    assert_eq!(evicted(&mut list, 50), 0);
    assert_eq!(evicted(&mut list, 51), 32);
    assert!(list.page(1).is_none(), "the emptied page closed");
    assert_eq!(evicted(&mut list, 52), 0);
    assert_eq!(list.pages.iter().flatten().count(), 1, "one page left");
    assert_eq!(evicted(&mut list, 101), 0);
    assert_eq!(evicted(&mut list, 201), 6);
    assert!(list.placement(items[0]).is_some(), "pinned");
    assert!(list.placement(items[1]).is_some(), "drawn at 50");

    let (mut list, items) = setup(512);
    assert_eq!(evicted(&mut list, 5), 0);
    assert_eq!(evicted(&mut list, 6), 38, "uncapped past four times");
    assert!(list.placement(items[0]).is_some());
}

/// A list on its first page at its first edge and under its budget ages nothing out, however
/// idle: a 32² page of four 16² items under a budget of a page.
#[test]
fn a_list_at_its_first_page_ages_nothing_out() {
    let mut list = list(32, 64, 1 << 20);
    let _ = insert(&mut list, 4, 16);
    assert_eq!(decay(&mut list, 10_000, ROOMY).evicted.len(), 0);
}

/// The emptiest page drains into the others once its items fit half their free texels, and closes
/// once empty; its items keep their handles, a pinned one among them. Four 16² items fill the
/// first 32² page and a fifth opens the second; while the first is full nothing drains. Two of the
/// first page's go: its 512 free texels hold the second page's 256 twice, so the second drains,
/// its one pinned item moving to the first page's free (16, 0), and closes.
#[test]
fn the_emptiest_page_drains_into_the_others() {
    let mut list = list(32, 32, 1 << 20);
    let first = insert(&mut list, 4, 16);
    let fifth = insert(&mut list, 1, 16)[0];
    list.pin(fifth);
    assert!(decay(&mut list, 1, ROOMY).moves.is_empty(), "no room");
    assert!(list.page(1).is_some());
    list.release(first[1]);
    list.release(first[2]);
    let moves = decay(&mut list, 2, ROOMY).moves;
    assert_eq!(
        moves,
        [Move {
            from: placement(1, 0, 0),
            from_epoch: 1,
            to: placement(0, 16, 0),
        }]
    );
    assert_eq!(at(&list, fifth), (0, U16Vec2::new(16, 0)));
    assert!(list.page(1).is_none(), "closed once empty");
    assert_eq!(list.page(0).unwrap().live_texels(), 3 * 16 * 16);
}

/// A 16² placement on `page` at `(x, y)`.
const fn placement(page: u8, x: u16, y: u16) -> Placement {
    Placement {
        page,
        origin: U16Vec2::new(x, y),
        size: U16Vec2::splat(16),
    }
}

/// A drain moves within the frame's budget and goes on the next frame, every move from the
/// draining page to another, so no page is a source and a destination in one frame. Sixteen 16²
/// items fill the first 64² page, four more open the second; thirteen of the first go, leaving its
/// three, 768 texels, the emptiest, which fit half the second's 3072 free. Under a budget of texels
/// short of one item the drain starts and moves nothing; under a budget of two copies it moves two,
/// and the next frame the third, and the page closes.
#[test]
fn a_drain_moves_within_the_budget_frame_by_frame() {
    let mut list = list(64, 64, 1 << 20);
    let first = insert(&mut list, 16, 16);
    let second = insert(&mut list, 4, 16);
    assert!(second.iter().all(|&handle| at(&list, handle).0 == 1));
    for &handle in &first[..13] {
        list.release(handle);
    }
    let two = MoveBudget {
        texels: u64::MAX,
        copies: 2,
    };
    let under = MoveBudget {
        texels: 16 * 16 - 1,
        copies: u32::MAX,
    };
    assert!(decay(&mut list, 1, under).moves.is_empty());
    assert_eq!(list.page(0).unwrap().state(), PageState::Draining);
    let moves = decay(&mut list, 2, two).moves;
    assert_eq!(moves.len(), 2);
    assert!(moves.iter().all(|m| m.from.page == 0 && m.to.page == 1));
    assert!(list.page(0).is_some());
    let moves = decay(&mut list, 3, two).moves;
    assert_eq!(moves.len(), 1);
    assert!(list.page(0).is_none(), "closed once empty");
    assert!(first[13..].iter().all(|&handle| at(&list, handle).0 == 1));
}

/// One page under a quarter full for an expiry window drains into a page at the smallest
/// power-of-two edge from the first that holds twice its items, landing at half. Five 32² items
/// take the first page from 32 through 64 to 128; three go, and the two left, 2048 texels, are
/// under a quarter of 128². Sparse from frame 1, the window ends at frame 120: a 64² page,
/// `2 · 2048 = 4096` texels, opens in the next slot, the two items move there, and the 128² page
/// closes. A list whose items fill a quarter never shrinks.
#[test]
fn a_sparse_page_shrinks_to_twice_its_items_after_a_window() {
    let mut list = list(32, 128, 1 << 20);
    let items = insert(&mut list, 5, 32);
    assert_eq!(list.page(0).unwrap().edge(), 128);
    for &handle in &items[2..] {
        list.release(handle);
    }
    for frame in 1..120 {
        assert!(decay(&mut list, frame, ROOMY).moves.is_empty(), "{frame}");
    }
    let moves = decay(&mut list, 120, ROOMY).moves;
    assert_eq!(moves.len(), 2);
    assert!(list.page(0).is_none(), "the large page closed");
    let page = list.page(1).unwrap();
    assert_eq!(page.edge(), 64);
    assert_eq!(page.live_texels() * 2, 64 * 64, "half full");
    assert_eq!(at(&list, items[0]), (1, U16Vec2::ZERO));

    let mut full = super::list(32, 128, 1 << 20);
    let items = insert(&mut full, 5, 32);
    full.release(items[4]);
    assert_eq!(full.page(0).unwrap().live_texels() * 4, 128 * 128);
    for frame in 1..=2 * 120 {
        assert!(decay(&mut full, frame, ROOMY).moves.is_empty());
    }
    assert_eq!(full.page(0).unwrap().edge(), 128);
}

/// A drain whose item fits the others' free texels but no shape of them stalls, and is not tried
/// again while what it counts holds. Four 32² items fill a 64² page in two 32-texel shelves; an 8 ×
/// 48 item opens a second page; three of the first page's go, leaving 3072 free texels in shelves
/// 32 high. The second page, 384 texels, is the emptiest and drains, but its item finds no shelf
/// 48 high: the drain stalls and the page takes allocations again. The next frame, under a budget
/// of no copy, a drain begun would leave the page draining; it stays open. Once the first page's
/// last item goes, the count moves and the planning runs again: the emptied first page drains,
/// with nothing to move, and closes.
#[test]
fn a_drain_that_finds_no_room_stalls_until_its_count_moves() {
    let mut list = list(64, 64, 1 << 20);
    let first = insert(&mut list, 4, 32);
    let mut evictions = Evictions::default();
    let thin = list
        .allocate(Tenant::Glyphs, U16Vec2::new(8, 48), 0, &mut evictions)
        .unwrap();
    assert_eq!(at(&list, thin).0, 1);
    for &handle in &first[1..] {
        list.release(handle);
    }
    assert!(decay(&mut list, 1, ROOMY).moves.is_empty());
    assert_eq!(list.page(1).unwrap().state(), PageState::Open, "cancelled");
    assert_eq!(
        list.stalled,
        Some(DrainCount {
            epoch: 1,
            live_texels: 8 * 48,
            others_free: 64 * 64 - 32 * 32,
        })
    );
    let none = MoveBudget {
        texels: u64::MAX,
        copies: 0,
    };
    let _ = decay(&mut list, 2, none);
    assert_eq!(
        list.page(1).unwrap().state(),
        PageState::Open,
        "not tried again"
    );
    list.release(first[0]);
    let _ = decay(&mut list, 3, none);
    assert!(
        list.page(0).is_none(),
        "the emptied first page drained and closed"
    );
}

/// No drain starts while moves the GPU half has not copied wait, so no page those moves land on
/// drains, which would close it under them before the commit made its texture take them. Three
/// 64² pages hold one, one and four pinned 16² items; a frame's decay drains the first into the
/// second, which then holds two, the emptiest. A purge before the commit leaves it, every move
/// landing on an open page; once the moves are taken, a purge drains it into the third.
#[test]
fn no_drain_starts_while_moves_wait_for_the_commit() {
    let mut list = list(64, 64, 1 << 20);
    let first = insert(&mut list, 16, 16);
    let second = insert(&mut list, 16, 16);
    let third = insert(&mut list, 4, 16);
    for &handle in first[1..].iter().chain(&second[1..]) {
        list.release(handle);
    }
    for &handle in [first[0], second[0]].iter().chain(&third) {
        list.pin(handle);
    }
    let mut moves = decay(&mut list, 1, ROOMY).moves;
    assert_eq!(moves.len(), 1, "the first page's item");
    assert_eq!(moves[0].to.page, 1);
    let mut evictions = Evictions::default();
    list.purge(1, &mut evictions, &mut moves);
    assert_eq!(moves.len(), 1, "no drain while the move waits");
    assert!(
        moves
            .iter()
            .all(|m| list.page(usize::from(m.to.page)).is_some())
    );
    moves.clear();
    list.purge(1, &mut evictions, &mut moves);
    assert_eq!(moves.len(), 2, "the second page's two items");
    assert!(list.page(1).is_none());
    assert!(moves.iter().all(|m| m.to.page == 2));
}
