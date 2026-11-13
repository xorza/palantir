use crate::gpu::resource::shelf_packer::{ShelfPacker, takes};
use crate::internals::seeded_rng::SeededRng;
use glam::U16Vec2;

fn size(w: u16, h: u16) -> U16Vec2 {
    U16Vec2::new(w, h)
}

/// Rasters fill a shelf left to right and open the next one under it, rounded up to four rows;
/// a raster too tall for a shelf's slack opens its own; one past the edge never fits.
#[test]
fn shelves_fill_left_to_right_and_stack() {
    let mut packer = ShelfPacker::new(64);
    assert_eq!(packer.allocate(size(30, 10)), Some(U16Vec2::new(0, 0)));
    assert_eq!(packer.allocate(size(30, 9)), Some(U16Vec2::new(30, 0)));
    assert_eq!(packer.shelf(0).height, 12, "10 rounds up to 12");
    assert_eq!(
        packer.allocate(size(10, 12)),
        Some(U16Vec2::new(0, 12)),
        "4 columns are left on the first shelf, too few for 10: a second opens"
    );
    // 2 rows above a 10 is within its slack of 4; 12 above a 20 is not.
    assert!(takes(12, 10));
    assert!(!takes(32, 20));
    assert_eq!(packer.allocate(size(10, 30)), Some(U16Vec2::new(0, 24)));
    assert_eq!(packer.allocate(size(65, 1)), None, "wider than the side");
    assert_eq!(
        packer.allocate(size(10, 16)),
        None,
        "no row left for a new shelf"
    );
}

/// A freed rectangle is room again at once, merged with the free spans beside it; the last
/// shelf emptied gives its rows back, so another height can open there.
#[test]
fn a_freed_rectangle_is_room_at_once() {
    let mut packer = ShelfPacker::new(64);
    let a = packer.allocate(size(20, 8)).unwrap();
    let b = packer.allocate(size(20, 8)).unwrap();
    assert_eq!(packer.allocate(size(20, 8)), Some(U16Vec2::new(40, 0)));
    assert_eq!(
        packer.allocate(size(40, 8)),
        Some(U16Vec2::new(0, 8)),
        "4 columns are left on the first shelf"
    );
    packer.deallocate(b, 20);
    assert_eq!(
        packer.allocate(size(24, 8)),
        Some(U16Vec2::new(40, 8)),
        "b's 20 columns are too few for 24; the second shelf's 24 are not"
    );
    packer.deallocate(a, 20);
    assert_eq!(
        packer.allocate(size(40, 8)),
        Some(U16Vec2::new(0, 0)),
        "a and b merged into one 40-wide span"
    );
    packer.deallocate(U16Vec2::new(0, 8), 40);
    packer.deallocate(U16Vec2::new(40, 8), 24);
    assert_eq!(
        packer.allocate(size(10, 30)),
        Some(U16Vec2::new(0, 8)),
        "the emptied last shelf gave its rows back"
    );
}

/// A grow keeps every placement and widens every shelf.
#[test]
fn a_grow_widens_each_shelf_and_keeps_placements() {
    let mut packer = ShelfPacker::new(32);
    assert_eq!(packer.allocate(size(32, 8)), Some(U16Vec2::new(0, 0)));
    assert_eq!(packer.allocate(size(8, 8)), Some(U16Vec2::new(0, 8)));
    packer.grow(64);
    assert_eq!(packer.allocate(size(32, 8)), Some(U16Vec2::new(32, 0)));
    assert_eq!(packer.allocate(size(56, 8)), Some(U16Vec2::new(8, 8)));
    packer.deallocate(U16Vec2::new(0, 0), 32);
    packer.deallocate(U16Vec2::new(32, 0), 32);
    assert_eq!(
        packer.allocate(size(64, 6)),
        Some(U16Vec2::new(0, 0)),
        "the emptied first shelf takes a raster within its slack"
    );
}

/// Two shelves emptied side by side merge into one, which a rectangle taller than either takes
/// once no unclaimed row is left, split to its height: the rows past it stay an empty shelf.
/// Without the merge the 16-row rectangle finds no room; the 24 merged rows are past its slack,
/// so it takes them by the split. A shelf emptied at the bottom gives its rows back, and a clear
/// frees every row.
#[test]
fn emptied_shelves_merge_and_split_to_a_taller_rectangle() {
    let mut packer = ShelfPacker::new(64);
    let a = packer.allocate(size(64, 12)).unwrap();
    let b = packer.allocate(size(64, 12)).unwrap();
    assert_eq!(packer.allocate(size(64, 40)), Some(U16Vec2::new(0, 24)));
    assert_eq!(packer.allocate(size(8, 16)), None, "no row left");
    packer.deallocate(a, 64);
    packer.deallocate(b, 64);
    assert_eq!(
        rows(&packer),
        [(0, 24), (24, 40)],
        "the two 12-row shelves merged"
    );
    assert_eq!(packer.allocate(size(8, 16)), Some(U16Vec2::new(0, 0)));
    assert_eq!(
        rows(&packer),
        [(0, 16), (16, 8), (24, 40)],
        "split to the rectangle's 16 rows, 8 left empty"
    );
    packer.deallocate(U16Vec2::new(0, 24), 64);
    assert_eq!(
        (rows(&packer), packer.top),
        (vec![(0, 16)], 16),
        "the 40-row shelf merged with the 8 empty rows above it, then went back to the unclaimed rows"
    );
    assert_eq!(packer.allocate(size(10, 30)), Some(U16Vec2::new(0, 16)));

    packer.clear();
    assert_eq!((rows(&packer), packer.top), (vec![], 0));
    assert_eq!(packer.allocate(size(64, 64)), Some(U16Vec2::ZERO));
}

/// With no unclaimed row, a rectangle takes the shortest empty shelf that holds it, not the
/// first.
#[test]
fn a_rectangle_takes_the_shortest_empty_shelf_that_holds_it() {
    let mut packer = ShelfPacker::new(64);
    let tall = packer.allocate(size(64, 32)).unwrap();
    packer.allocate(size(64, 8)).unwrap();
    let short = packer.allocate(size(64, 20)).unwrap();
    packer.allocate(size(64, 4)).unwrap();
    packer.deallocate(tall, 64);
    packer.deallocate(short, 64);
    assert_eq!(
        packer.allocate(size(8, 18)),
        Some(short),
        "the 20-row shelf"
    );
    assert_eq!(rows(&packer)[0], (0, 32), "the 32-row one kept whole");
}

/// The rule a shelf class keeps: a rectangle goes on a shelf no lower than it and at most a
/// quarter of its height taller, four rows at least. A 4-row rectangle takes shelves of 4 to 8
/// rows, a 20-row one 20 to 25, a 40-row one 40 to 50.
#[test]
fn a_shelf_takes_a_rectangle_within_its_slack() {
    for (height, h, want) in [
        (4, 4, true),
        (8, 4, true),
        (9, 4, false),
        (3, 4, false),
        (25, 20, true),
        (26, 20, false),
        (19, 20, false),
        (50, 40, true),
        (51, 40, false),
        (5, 1, true),
        (6, 1, false),
    ] {
        assert_eq!(takes(height, h), want, "{h} on {height}");
    }
}

/// The lowest shelf that takes a rectangle and has room wins over a taller one above it, and of
/// two shelves of one height the upper. A 12-row rectangle, which 12- to 16-row shelves take,
/// goes on the upper 12-row shelf, then on the lower, and once both are full on the 16-row one
/// above them.
#[test]
fn the_lowest_shelf_with_room_wins_then_the_upper() {
    let mut packer = ShelfPacker::new(64);
    packer.allocate(size(60, 13)).unwrap();
    packer.allocate(size(60, 11)).unwrap();
    packer.allocate(size(60, 12)).unwrap();
    assert_eq!(
        rows(&packer),
        [(0, 16), (16, 12), (28, 12)],
        "an 11-row rectangle is past the 16-row shelf's slack, a 12-row one past the first \
         12-row shelf's room"
    );
    assert_eq!(packer.allocate(size(4, 12)), Some(U16Vec2::new(60, 16)));
    assert_eq!(packer.allocate(size(4, 12)), Some(U16Vec2::new(60, 28)));
    assert_eq!(packer.allocate(size(4, 12)), Some(U16Vec2::new(60, 0)));
}

/// A free merges with the span before it, the span after it, both, or neither, and the shelf's
/// widest span follows each. A carve of the widest span looks for the next widest; a carve of a
/// narrower one leaves it.
#[test]
fn a_free_merges_with_its_neighbors_and_the_widest_follows() {
    let mut packer = ShelfPacker::new(64);
    let rects: Vec<_> = (0..8)
        .map(|_| packer.allocate(size(8, 4)).unwrap())
        .collect();
    let widest = |packer: &ShelfPacker| packer.shelves[0].widest;
    assert_eq!((widest(&packer), spans(&packer, 0)), (0, vec![]), "full");
    packer.deallocate(rects[1], 8);
    assert_eq!(
        (widest(&packer), spans(&packer, 0)),
        (8, vec![(8, 16)]),
        "neither"
    );
    packer.deallocate(rects[2], 8);
    assert_eq!(
        (widest(&packer), spans(&packer, 0)),
        (16, vec![(8, 24)]),
        "before"
    );
    packer.deallocate(rects[0], 8);
    assert_eq!(
        (widest(&packer), spans(&packer, 0)),
        (24, vec![(0, 24)]),
        "after"
    );
    packer.deallocate(rects[4], 8);
    packer.deallocate(rects[3], 8);
    assert_eq!(
        (widest(&packer), spans(&packer, 0)),
        (40, vec![(0, 40)]),
        "both"
    );
    packer.deallocate(rects[6], 8);
    assert_eq!(spans(&packer, 0), [(0, 40), (48, 56)]);
    assert_eq!(packer.allocate(size(8, 4)), Some(U16Vec2::new(0, 0)));
    assert_eq!(
        (widest(&packer), spans(&packer, 0)),
        (32, vec![(8, 40), (48, 56)]),
        "the widest carved: the next widest is what is left of it"
    );
    assert_eq!(packer.allocate(size(32, 4)), Some(U16Vec2::new(8, 0)));
    assert_eq!(
        (widest(&packer), spans(&packer, 0)),
        (8, vec![(48, 56)]),
        "the widest used up: the narrower one is the widest"
    );
    packer.deallocate(U16Vec2::new(8, 0), 32);
    assert_eq!(packer.allocate(size(4, 4)), Some(U16Vec2::new(8, 0)));
    assert_eq!(
        (widest(&packer), spans(&packer, 0)),
        (28, vec![(12, 40), (48, 56)]),
        "the widest carved again"
    );
    packer.deallocate(U16Vec2::ZERO, 8);
    assert_eq!(packer.allocate(size(8, 4)), Some(U16Vec2::ZERO));
    assert_eq!(
        (widest(&packer), spans(&packer, 0)),
        (28, vec![(12, 40), (48, 56)]),
        "a narrower span carved: the widest stays"
    );
}

/// The edges of the side: a rectangle as large as the side fills it and nothing else goes; one a
/// texel past it on either axis never fits; a 1² side holds one texel on a 1-row shelf; a side of
/// no texels holds nothing. A side at the packer's 16-bit edge opens a shelf for a rectangle as
/// tall as it, which no four-row rounding can hold, and frees back to empty.
#[test]
fn rectangles_at_the_edges_of_the_side() {
    let mut packer = ShelfPacker::new(64);
    assert_eq!(packer.allocate(size(65, 1)), None);
    assert_eq!(packer.allocate(size(1, 65)), None);
    assert_eq!(packer.allocate(size(64, 64)), Some(U16Vec2::ZERO));
    assert_eq!(packer.allocate(size(1, 1)), None, "the side is full");
    packer.deallocate(U16Vec2::ZERO, 64);
    assert_eq!((rows(&packer), packer.top), (vec![], 0), "empty again");

    let mut one = ShelfPacker::new(1);
    assert_eq!(one.allocate(size(1, 1)), Some(U16Vec2::ZERO));
    assert_eq!(rows(&one), [(0, 1)], "four rows held to the side's one");
    assert_eq!(one.allocate(size(1, 1)), None);

    assert_eq!(ShelfPacker::within(0).allocate(size(1, 1)), None);

    let mut widest = ShelfPacker::within(1 << 20);
    assert_eq!(widest.edge, u16::MAX, "held to the 16-bit edge");
    assert_eq!(widest.allocate(size(3, u16::MAX - 1)), Some(U16Vec2::ZERO));
    assert_eq!(rows(&widest), [(0, u16::MAX)]);
    assert_eq!(
        widest.allocate(size(u16::MAX - 3, u16::MAX)),
        Some(U16Vec2::new(3, 0))
    );
    widest.deallocate(U16Vec2::ZERO, 3);
    widest.deallocate(U16Vec2::new(3, 0), u16::MAX - 3);
    assert_eq!((rows(&widest), widest.top), (vec![], 0));
}

/// A grow gives a full shelf a span of the new columns and widens a shelf's last span where it
/// reaches the old edge, and an empty shelf stays empty at the new edge: it still merges with an
/// empty neighbor and gives its rows back.
#[test]
fn an_empty_shelf_stays_empty_across_a_grow() {
    let mut packer = ShelfPacker::new(32);
    let a = packer.allocate(size(32, 8)).unwrap();
    let b = packer.allocate(size(8, 8)).unwrap();
    let c = packer.allocate(size(8, 12)).unwrap();
    assert_eq!(rows(&packer), [(0, 8), (8, 8), (16, 12)]);
    packer.deallocate(b, 8);
    assert!(packer.is_empty(&packer.shelves[1]));
    packer.grow(64);
    assert_eq!(
        spans(&packer, 0),
        [(32, 64)],
        "the full shelf gains the new columns"
    );
    assert_eq!(spans(&packer, 2), [(8, 64)], "a last span widened");
    assert!(packer.is_empty(&packer.shelves[1]), "empty at the new edge");
    packer.deallocate(a, 32);
    assert_eq!(rows(&packer), [(0, 16), (16, 12)], "both empty: merged");
    packer.deallocate(c, 8);
    assert_eq!(
        (rows(&packer), packer.top),
        (vec![], 0),
        "all empty: the rows given back"
    );
}

/// A clear frees every row and keeps each shelf's span list, so the shelves it opens next
/// allocate nothing.
#[test]
fn a_clear_keeps_every_span_list() {
    let mut packer = ShelfPacker::new(64);
    for h in [4, 8, 16] {
        packer.allocate(size(10, h)).unwrap();
    }
    packer.clear();
    assert_eq!(
        (rows(&packer), packer.top, packer.spare.len()),
        (vec![], 0, 3)
    );
    packer.allocate(size(10, 4)).unwrap();
    assert_eq!(packer.spare.len(), 2, "the new shelf took a kept list");

    // A reset clears as well and takes the new edge: 100 texels wide fits 128, not 64.
    assert_eq!(packer.allocate(size(100, 4)), None);
    packer.reset(128);
    assert_eq!(
        (rows(&packer), packer.top, packer.spare.len(), packer.edge),
        (vec![], 0, 3, 128)
    );
    assert_eq!(packer.allocate(size(100, 4)), Some(U16Vec2::ZERO));
}

/// How a workload frees what it placed.
#[derive(Clone, Copy, Debug)]
enum Eviction {
    /// The oldest first, as a cache that ages its entries does.
    Oldest,
    /// One picked at random, as an atlas that evicts what was drawn least lately does.
    Random,
}

/// A run of placements and frees: the side it starts at and the one it grows to halfway, the
/// sizes it places, how often a step frees, and in what order.
#[derive(Clone, Copy, Debug)]
struct Workload {
    name: &'static str,
    side: u16,
    grown: Option<u16>,
    sizes: fn(&mut SeededRng) -> U16Vec2,
    /// Out of 100 steps, how many free a rectangle.
    frees: u32,
    eviction: Eviction,
    /// Whether some rectangle finds no row left and takes an emptied shelf.
    reuses_empty: bool,
}

/// Where the reference places a rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Path {
    /// On a shelf that takes it.
    Shelf,
    /// On a new shelf.
    New,
    /// On an emptied shelf, split to it.
    Empty,
}

/// Every workload's run, step by step against a reference that finds by a full scan where the
/// packer must place each rectangle, or that it must refuse it, and against the invariants
/// [`check`] holds. A run then frees what is left, in its own order, and the packer is empty: no
/// shelf and no claimed row. Each workload places on shelves and on new ones and refuses, and
/// the ones that run out of rows place on emptied shelves.
#[test]
fn every_workload_places_as_the_reference_and_drains_to_empty() {
    const STEPS: usize = 3000;
    let workloads = [
        Workload {
            name: "small rectangles of every size, grown halfway",
            side: 32,
            grown: Some(64),
            sizes: |rng| size(rng.within(1, 20), rng.within(1, 20)),
            frees: 33,
            eviction: Eviction::Random,
            reuses_empty: true,
        },
        Workload {
            name: "glyphs of one shelf class, whose spans split and merge",
            side: 128,
            grown: None,
            sizes: |rng| size(rng.within(3, 12), rng.within(13, 16)),
            frees: 45,
            eviction: Eviction::Random,
            reuses_empty: false,
        },
        Workload {
            name: "tiny rectangles among ones near the side",
            side: 64,
            grown: Some(128),
            sizes: |rng| {
                if rng.below(8) == 0 {
                    size(rng.within(40, 64), rng.within(20, 64))
                } else {
                    size(rng.within(1, 4), rng.within(1, 4))
                }
            },
            frees: 40,
            eviction: Eviction::Random,
            reuses_empty: true,
        },
        Workload {
            name: "whole rows, freed oldest first",
            side: 64,
            grown: None,
            sizes: |rng| size(64, rng.within(1, 16)),
            frees: 50,
            eviction: Eviction::Oldest,
            reuses_empty: true,
        },
        Workload {
            name: "tall rectangles, which split emptied shelves",
            side: 64,
            grown: None,
            sizes: |rng| size(rng.within(1, 64), rng.within(24, 64)),
            frees: 50,
            eviction: Eviction::Random,
            reuses_empty: true,
        },
        Workload {
            name: "a burst that fills the side, then frees oldest first",
            side: 48,
            grown: Some(96),
            sizes: |rng| size(rng.within(2, 24), rng.within(2, 24)),
            frees: 20,
            eviction: Eviction::Oldest,
            reuses_empty: true,
        },
    ];
    for workload in workloads {
        let name = workload.name;
        let mut packer = ShelfPacker::new(workload.side);
        let mut placed: Vec<(U16Vec2, U16Vec2)> = Vec::new();
        let mut rng = SeededRng::new(0x2545_f491_4f6c_dd1d);
        let mut paths = [0; 3];
        let mut refusals = 0;
        for step in 0..STEPS {
            if step == STEPS / 2
                && let Some(side) = workload.grown
            {
                packer.grow(side);
            }
            if rng.below(100) < u64::from(workload.frees) && !placed.is_empty() {
                let at = match workload.eviction {
                    Eviction::Oldest => 0,
                    Eviction::Random => rng.below(placed.len() as u64) as usize,
                };
                let (origin, size) = placed.remove(at);
                packer.deallocate(origin, size.x);
            } else {
                let size = (workload.sizes)(&mut rng);
                let want = reference(&packer, size);
                let got = packer.allocate(size);
                assert_eq!(
                    got,
                    want.map(|(origin, _)| origin),
                    "{name}, step {step}: {size}"
                );
                match want {
                    Some((origin, path)) => {
                        paths[path as usize] += 1;
                        placed.push((origin, size));
                    }
                    None => refusals += 1,
                }
            }
            check(&packer, &placed);
        }
        let [on_shelf, new, empty] = paths;
        assert!(
            on_shelf + new + empty > 500
                && on_shelf > 0
                && new > 0
                && refusals > 0
                && (empty > 0) == workload.reuses_empty,
            "{name}: {on_shelf} on a shelf, {new} on a new one, {empty} on an emptied one, \
             {refusals} refused"
        );
        while !placed.is_empty() {
            let at = match workload.eviction {
                Eviction::Oldest => 0,
                Eviction::Random => rng.below(placed.len() as u64) as usize,
            };
            let (origin, size) = placed.remove(at);
            packer.deallocate(origin, size.x);
            check(&packer, &placed);
        }
        assert_eq!((rows(&packer), packer.top), (vec![], 0), "{name}: drained");
    }
}

/// Where `packer` must place a rectangle of `size`, found by a full scan of its spans: on the
/// lowest shelf that takes it with a span as wide, the upper of two of one height, at that
/// shelf's first such span; else on a new shelf at the first unclaimed row; else at the start of
/// the shortest empty shelf that holds it, the upper of two. `None` when none is.
fn reference(packer: &ShelfPacker, size: U16Vec2) -> Option<(U16Vec2, Path)> {
    let (w, h) = (size.x, size.y);
    if w > packer.edge || h > packer.edge {
        return None;
    }
    let fitting = packer
        .shelves
        .iter()
        .filter(|shelf| shelf.takes(h))
        .filter_map(|shelf| {
            let span = shelf.free.iter().find(|span| span.end - span.start >= w)?;
            Some((shelf.height, shelf.y, span.start))
        })
        .min();
    if let Some((_, y, x)) = fitting {
        return Some((U16Vec2::new(x, y), Path::Shelf));
    }
    if packer.edge - packer.top >= h {
        return Some((U16Vec2::new(0, packer.top), Path::New));
    }
    packer
        .shelves
        .iter()
        .filter(|shelf| h <= shelf.height && matches!(shelf.free.as_slice(), [span] if span.start == 0 && span.end == packer.edge))
        .map(|shelf| (shelf.height, shelf.y))
        .min()
        .map(|(_, y)| (U16Vec2::new(0, y), Path::Empty))
}

/// The rectangles of `placed` lie apart and inside the side, each at its shelf's row and no
/// taller than it, and each shelf's spans and rectangles tile its row as the packer promises.
fn check(packer: &ShelfPacker, placed: &[(U16Vec2, U16Vec2)]) {
    let side = usize::from(packer.edge);
    let mut owner = vec![false; side * side];
    for &(origin, size) in placed {
        let shelf = &packer.shelves[packer.shelf_at(origin.y)];
        assert_eq!(shelf.y, origin.y, "{origin} sits at its shelf's row");
        assert!(
            size.y <= shelf.height,
            "{size} fits its shelf of {}",
            shelf.height
        );
        for y in origin.y..origin.y + size.y {
            for x in origin.x..origin.x + size.x {
                let texel = &mut owner[usize::from(y) * side + usize::from(x)];
                assert!(!*texel, "texel ({x}, {y}) owned twice");
                *texel = true;
            }
        }
    }
    let mut y = 0;
    for shelf in &packer.shelves {
        assert_eq!(shelf.y, y, "shelves stack with no gap");
        y += shelf.height;
        let spans = &shelf.free;
        let widest = spans.iter().map(|span| span.end - span.start).max();
        assert_eq!(
            shelf.widest,
            widest.unwrap_or(0),
            "shelf {shelf:?} knows its widest span"
        );
        for pair in spans.windows(2) {
            assert!(
                pair[0].end < pair[1].start,
                "spans sorted and apart: {pair:?}"
            );
        }
        assert!(spans.iter().all(|span| !span.is_empty()), "no empty span");
        let mut row = spans.clone();
        row.extend(
            placed
                .iter()
                .filter(|(origin, _)| origin.y == shelf.y)
                .map(|(origin, size)| origin.x..origin.x + size.x),
        );
        row.sort_by_key(|columns| columns.start);
        let mut x = 0;
        for columns in row {
            assert_eq!(columns.start, x, "shelf {shelf:?} tiles its row");
            x = columns.end;
        }
        assert_eq!(x, packer.edge, "shelf {shelf:?} tiles its row");
    }
    assert_eq!(y, packer.top);
}

/// Each shelf's row and height, top to bottom.
fn rows(packer: &ShelfPacker) -> Vec<(u16, u16)> {
    packer
        .shelves()
        .iter()
        .map(|shelf| (shelf.y, shelf.height))
        .collect()
}

/// Shelf `i`'s free spans, as `(start, end)`.
fn spans(packer: &ShelfPacker, i: usize) -> Vec<(u16, u16)> {
    packer.shelves[i]
        .free
        .iter()
        .map(|span| (span.start, span.end))
        .collect()
}
