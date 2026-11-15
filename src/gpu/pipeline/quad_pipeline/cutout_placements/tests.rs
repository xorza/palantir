use crate::gpu::atlas::AtlasPool;
use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::atlas::atlas_handle::Placement;
use crate::gpu::atlas::tenant::Tenant;
use crate::gpu::pipeline::quad_pipeline::cutout_placements::{CutoutPlacements, CutoutPlacing};
use crate::gpu::pipeline::quad_pipeline::cutout_plan::{BakeTable, CutoutKey, CutoutPlan};
use glam::{U16Vec2, UVec2};

fn key(r: f32) -> CutoutKey {
    CutoutKey::new(r, 2.0)
}

/// A pool whose mask list holds one 64² page and evicts past nothing.
fn pool() -> AtlasPool {
    AtlasPool::uniform(64, 64, 0)
}

/// The code of a table of `side` at `(x, y)` of page 0.
fn at(x: u16, y: u16, side: u32) -> u32 {
    on(0, x, y, side)
}

/// The code of a table of `side` at `(x, y)` of page `page`.
fn on(page: u8, x: u16, y: u16, side: u32) -> u32 {
    CutoutPlan::code(Placement {
        page,
        origin: U16Vec2::new(x, y),
        size: U16Vec2::splat(side as u16),
    })
}

/// A key keeps its table while a plan uses it, pinned, and its uses count per plan: two uses
/// of `a` share one table at the origin, a second key lands beside it. Released, a key keeps
/// its table until the clock takes it for another tenant's insert: past the grace, a glyph
/// that needs the room evicts `b`, released earlier, and the drain drops its entry, so its
/// next use places it again, on a page the list opens, as nothing on the first is evictable;
/// `a`, still in use, keeps its place.
#[test]
fn a_table_stays_while_pinned_and_goes_to_the_clock_once_released() {
    let mut pool = pool();
    let mut placements = CutoutPlacements::default();
    let mut placing = CutoutPlacing {
        placements: &mut placements,
        pool: &mut pool,
    };
    assert_eq!(placing.acquire(key(1.0), 32), Some(at(0, 0, 32)));
    assert_eq!(placing.acquire(key(1.0), 32), Some(at(0, 0, 32)), "shared");
    assert_eq!(placing.users(key(1.0)), 2);
    assert_eq!(placing.acquire(key(2.0), 32), Some(at(32, 0, 32)));
    placing.release(key(2.0));
    assert_eq!(placing.users(key(2.0)), 0);
    assert_eq!(placing.uses(), 2);
    let b = table(&placing, 2.0, 32);
    placements.bakes(&[b], &mut Vec::new());
    assert_eq!(placements.baked_len(), 1);

    pool.advance_to(2);
    let glyph = pool
        .allocate(AtlasFormat::R8, Tenant::Glyphs.owner(), UVec2::new(64, 32))
        .unwrap()
        .handle;
    assert_eq!(pool.placement(glyph).unwrap().origin, U16Vec2::new(0, 32));
    assert!(
        pool.allocate(AtlasFormat::R8, Tenant::Glyphs.owner(), UVec2::splat(32))
            .is_ok(),
        "b's texels"
    );
    placements.drop_evicted(&mut pool);
    assert_eq!(placements.baked_len(), 0, "b's bake went with it");
    let mut placing = CutoutPlacing {
        placements: &mut placements,
        pool: &mut pool,
    };
    assert_eq!(placing.users(key(2.0)), 0, "b's entry dropped");
    assert_eq!(placing.code(key(1.0)), at(0, 0, 32), "a kept its place");
    assert_eq!(placing.acquire(key(2.0), 32), Some(on(1, 0, 0, 32)));
}

/// A key whose table the clock took before a drain is placed again at its next acquire, as
/// the lookup finds its handle stale; the drain then passes over the stale handle.
#[test]
fn an_acquire_after_an_eviction_places_the_table_again() {
    let mut pool = pool();
    let mut placements = CutoutPlacements::default();
    let mut placing = CutoutPlacing {
        placements: &mut placements,
        pool: &mut pool,
    };
    assert_eq!(placing.acquire(key(1.0), 64), Some(at(0, 0, 64)));
    placing.release(key(1.0));
    pool.advance_to(2);
    let glyph = pool
        .allocate(AtlasFormat::R8, Tenant::Glyphs.owner(), UVec2::splat(64))
        .unwrap()
        .handle;
    pool.release(glyph);
    let mut placing = CutoutPlacing {
        placements: &mut placements,
        pool: &mut pool,
    };
    assert_eq!(placing.acquire(key(1.0), 64), Some(at(0, 0, 64)));
    assert_eq!(placing.users(key(1.0)), 1);
    placements.drop_evicted(&mut pool);
    let placing = CutoutPlacing {
        placements: &mut placements,
        pool: &mut pool,
    };
    assert_eq!(placing.users(key(1.0)), 1, "the drain kept the new table");
}

/// The table of `key(r)` at its place, of side `side`, baked whole.
fn table(placing: &CutoutPlacing<'_>, r: f32, side: u32) -> BakeTable {
    BakeTable {
        table: placing.code(key(r)),
        r,
        sigma: 2.0,
        lo: U16Vec2::ZERO,
        hi: U16Vec2::splat(side as u16),
    }
}

/// Frame by frame: the first bakes both its tables; the second needs nothing new and bakes
/// nothing; the third widens `a`, which bakes the span that holds both; the fourth needs `b`
/// in part, already baked, and bakes nothing; and `b` moved by a drain, a new code at its new
/// place, bakes nothing either, as the commit carried its texels.
#[test]
fn a_table_bakes_only_what_it_lacks() {
    let mut pool = pool();
    let mut placements = CutoutPlacements::default();
    let mut placing = CutoutPlacing {
        placements: &mut placements,
        pool: &mut pool,
    };
    placing.acquire(key(1.0), 32).unwrap();
    placing.acquire(key(2.0), 32).unwrap();
    let span = |table: BakeTable, lo: (u16, u16), hi: (u16, u16)| BakeTable {
        lo: U16Vec2::new(lo.0, lo.1),
        hi: U16Vec2::new(hi.0, hi.1),
        ..table
    };
    let a = span(table(&placing, 1.0, 32), (0, 0), (10, 10));
    let b = span(table(&placing, 2.0, 32), (2, 2), (16, 16));
    let mut bakes = Vec::new();
    placements.bakes(&[a, b], &mut bakes);
    assert_eq!(bakes, [a, b]);
    placements.bakes(&[a, b], &mut bakes);
    assert_eq!(bakes, []);
    let wide_a = span(a, (4, 0), (16, 8));
    placements.bakes(&[wide_a, b], &mut bakes);
    assert_eq!(bakes, [span(a, (0, 0), (16, 10))]);
    placements.bakes(&[span(b, (4, 4), (8, 8))], &mut bakes);
    assert_eq!(bakes, []);

    let moved = BakeTable {
        table: at(0, 32, 32),
        ..b
    };
    placements.bakes(&[moved], &mut bakes);
    assert_eq!(bakes, [], "a move carries the texels");
}

/// A table a drain moved keeps its entry, its pin and its bake: its code is its new place's,
/// and it bakes nothing more, as the commit moved its texels. On pages of 64², `a`'s 32² table
/// pinned at the first page's origin, three glyphs filling the page and one opening the second;
/// with the three gone, the first page, the emptier by slot, drains `a` to the second's (32, 0).
#[test]
fn a_moved_table_keeps_its_bake_at_its_new_place() {
    let mut pool = pool();
    let mut placements = CutoutPlacements::default();
    let mut placing = CutoutPlacing {
        placements: &mut placements,
        pool: &mut pool,
    };
    assert_eq!(placing.acquire(key(1.0), 32), Some(at(0, 0, 32)));
    let a = table(&placing, 1.0, 32);
    placements.bakes(&[a], &mut Vec::new());
    let glyphs: Vec<_> = (0..4)
        .map(|_| {
            pool.allocate(AtlasFormat::R8, Tenant::Glyphs.owner(), UVec2::splat(32))
                .unwrap()
                .handle
        })
        .collect();
    assert_eq!(pool.placement(glyphs[3]).unwrap().page, 1);
    for &glyph in &glyphs[..3] {
        pool.release(glyph);
    }
    pool.advance_to(1);
    pool.end_submit();
    assert_eq!(pool.list(AtlasFormat::R8).moves().len(), 1);
    let placing = CutoutPlacing {
        placements: &mut placements,
        pool: &mut pool,
    };
    let moved = on(1, 32, 0, 32);
    assert_eq!(placing.code(key(1.0)), moved);
    assert_eq!(placing.users(key(1.0)), 1, "still pinned");
    let mut bakes = Vec::new();
    placements.bakes(&[BakeTable { table: moved, ..a }], &mut bakes);
    assert_eq!(bakes, [], "the move carried the texels");
}
