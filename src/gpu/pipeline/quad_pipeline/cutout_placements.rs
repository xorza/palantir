//! Where each `(r, σ)` cutout table lives in the pool's mask pages, and which of its texels are
//! baked.

use crate::gpu::atlas::AtlasPool;
use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::atlas::atlas_handle::{Allocation, AtlasHandle};
use crate::gpu::atlas::error::AtlasError;
use crate::gpu::atlas::tenant::Tenant;
use crate::gpu::pipeline::quad_pipeline::cutout_plan::{BakeTable, CutoutKey, CutoutPlan};
use glam::{U16Vec2, UVec2};
use rustc_hash::FxHashMap;

/// The table each cutout key holds in the pool's mask pages, which the backend lends each
/// window's [`CutoutPlan`] for the frame it plans, as glyphon lends one `TextAtlas` to each
/// `TextRenderer::prepare`.
///
/// A plan pins each key it tables, so a table keeps its place across frames and windows and is
/// baked once. A key no plan tables is left to the pool's clock, which may give its texels to
/// another tenant; its entry goes with it, its bake too. Refused: packing each window's tables
/// afresh every frame, which moved a table whenever a key before it came or went and made two
/// windows with different shadows bake over each other every frame.
#[derive(Debug, Default)]
pub(crate) struct CutoutPlacements {
    keys: FxHashMap<CutoutKey, PlacedTable>,
    /// The key each table's handle holds, so an eviction the pool queues finds its entry.
    by_handle: FxHashMap<AtlasHandle, CutoutKey>,
}

#[derive(Clone, Copy, Debug)]
struct PlacedTable {
    handle: AtlasHandle,
    /// Every texel baked since the table was placed; `None` before its first bake. A drain's move
    /// carries the texels with the table, so a moved table keeps its span.
    baked: Option<BakedSpan>,
}

/// The span of a table's texels baked so far, table-relative as a [`BakeTable`]'s.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BakedSpan {
    lo: U16Vec2,
    hi: U16Vec2,
}

impl BakedSpan {
    /// The span `table` bakes.
    const fn of(table: BakeTable) -> Self {
        Self {
            lo: table.lo,
            hi: table.hi,
        }
    }

    /// Whether the span holds `table`'s.
    fn holds(self, table: BakeTable) -> bool {
        self.lo.cmple(table.lo).all() && table.hi.cmple(self.hi).all()
    }

    /// `table` over the least span that holds its own and this one.
    fn joined(self, table: BakeTable) -> BakeTable {
        BakeTable {
            lo: self.lo.min(table.lo),
            hi: self.hi.max(table.hi),
            ..table
        }
    }
}

/// [`CutoutPlacements`] with the pool its tables live in, as a plan places them.
#[derive(Debug)]
pub(crate) struct CutoutPlacing<'a> {
    pub(crate) placements: &'a mut CutoutPlacements,
    pub(crate) pool: &'a mut AtlasPool,
}

impl CutoutPlacing<'_> {
    /// Take a use of `key`'s table of `side` texels, which pins it: its [`CutoutPlan::code`],
    /// placed now where it has none or its last place was evicted. `None` when the mask pages
    /// hold no room for it, the pool full of what is drawn or pinned.
    pub(crate) fn acquire(&mut self, key: CutoutKey, side: u32) -> Option<u32> {
        let placements = &mut *self.placements;
        if let Some(placed) = placements.keys.get(&key) {
            if let Some(placement) = self.pool.pin(placed.handle) {
                return Some(CutoutPlan::code(placement));
            }
            // Evicted, its handle waiting in the queue, which the drain passes over.
            placements.by_handle.remove(&placed.handle);
            placements.keys.remove(&key);
        }
        let Allocation { handle, placement } =
            match self
                .pool
                .allocate(AtlasFormat::R8, Tenant::CutoutTables, UVec2::splat(side))
            {
                Ok(allocation) => allocation,
                Err(AtlasError::Full) => return None,
                Err(oversized @ AtlasError::Oversized { .. }) => {
                    unreachable!("a table fits a mask page by the plan's bound: {oversized}")
                }
            };
        let pinned = self.pool.pin(handle);
        debug_assert_eq!(pinned, Some(placement), "a new table is placed");
        placements.keys.insert(
            key,
            PlacedTable {
                handle,
                baked: None,
            },
        );
        placements.by_handle.insert(handle, key);
        Some(CutoutPlan::code(placement))
    }

    /// The [`CutoutPlan::code`] of `key`, which a plan holds a use of.
    pub(crate) fn code(&self, key: CutoutKey) -> u32 {
        let placement = self
            .pool
            .placement(self.placements.keys[&key].handle)
            .expect("a held table is pinned");
        CutoutPlan::code(placement)
    }

    /// Give back one use of `key`, which [`Self::acquire`] placed; the unpin stamps it as drawn
    /// now, so the clock takes the tables let go longest ago first.
    pub(crate) fn release(&mut self, key: CutoutKey) {
        self.pool.unpin(self.placements.keys[&key].handle);
    }
}

impl CutoutPlacements {
    /// Write into `bakes` what of the frame's `tables` the mask pages lack, and note it baked: a
    /// table baked whole bakes nothing, one baked in part bakes the least span that holds both,
    /// and one placed since its last bake, a new entry, bakes its span. A drain's move keeps the
    /// entry and its span, as the commit moved its texels.
    pub(crate) fn bakes(&mut self, tables: &[BakeTable], bakes: &mut Vec<BakeTable>) {
        bakes.clear();
        for &table in tables {
            let placed = self
                .keys
                .get_mut(&table.key())
                .expect("a planned table is placed");
            let bake = match placed.baked {
                Some(baked) if baked.holds(table) => continue,
                Some(baked) => baked.joined(table),
                None => table,
            };
            placed.baked = Some(BakedSpan::of(bake));
            bakes.push(bake);
        }
    }

    /// Drop the tables the pool evicted since the last drain: at the end of each submit, and after
    /// a purge.
    pub(crate) fn drop_evicted(&mut self, pool: &mut AtlasPool) {
        for handle in pool.drain_evicted(Tenant::CutoutTables) {
            // A lookup that found the table gone dropped its entry already.
            if let Some(key) = self.by_handle.remove(&handle) {
                self.keys.remove(&key);
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::gpu::atlas::atlas_format::AtlasFormat;
    use crate::gpu::pipeline::quad_pipeline::cutout_placements::{CutoutPlacements, CutoutPlacing};
    use crate::gpu::pipeline::quad_pipeline::cutout_plan::CutoutKey;

    impl CutoutPlacing<'_> {
        /// The plans that hold a use of `key`, none where it is not placed.
        pub(crate) fn users(&self, key: CutoutKey) -> u32 {
            self.placements.keys.get(&key).map_or(0, |placed| {
                self.pool
                    .list(AtlasFormat::R8)
                    .pins(placed.handle)
                    .unwrap_or(0)
            })
        }

        /// The uses every plan holds, of every key.
        pub(crate) fn uses(&self) -> u32 {
            let list = self.pool.list(AtlasFormat::R8);
            self.placements
                .keys
                .values()
                .filter_map(|placed| list.pins(placed.handle))
                .sum()
        }
    }

    impl CutoutPlacements {
        /// The tables baked in part or whole.
        pub(crate) fn baked_len(&self) -> usize {
            self.keys
                .values()
                .filter(|placed| placed.baked.is_some())
                .count()
        }
    }
}

#[cfg(test)]
mod tests {
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
            .allocate(AtlasFormat::R8, Tenant::Glyphs, UVec2::new(64, 32))
            .unwrap()
            .handle;
        assert_eq!(pool.placement(glyph).unwrap().origin, U16Vec2::new(0, 32));
        assert!(
            pool.allocate(AtlasFormat::R8, Tenant::Glyphs, UVec2::splat(32))
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
            .allocate(AtlasFormat::R8, Tenant::Glyphs, UVec2::splat(64))
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
                pool.allocate(AtlasFormat::R8, Tenant::Glyphs, UVec2::splat(32))
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
}
