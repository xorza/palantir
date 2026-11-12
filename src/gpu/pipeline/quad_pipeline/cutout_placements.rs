//! Where each `(r, σ)` cutout table lives in the pool's mask pages, and which of its texels are
//! baked.

use crate::gpu::atlas::AtlasPool;
use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::atlas::atlas_handle::AtlasHandle;
use crate::gpu::atlas::error::AtlasError;
use crate::gpu::atlas::tenant::Tenant;
use crate::gpu::pipeline::quad_pipeline::cutout_plan::{BakeTable, CutoutKey, CutoutPlan};
use glam::U16Vec2;
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
    side: u32,
    /// The last bake into the table, its span every texel baked since it was placed; `None`
    /// before its first.
    baked: Option<BakeTable>,
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
            if let Some(placement) = self.pool.placement(placed.handle) {
                self.pool.pin(placed.handle);
                return Some(CutoutPlan::code(placement, side));
            }
            // Evicted, its handle waiting in the queue, which the drain passes over.
            placements.by_handle.remove(&placed.handle);
            placements.keys.remove(&key);
        }
        let edge = u16::try_from(side).expect("a table's side is at most a page's");
        let handle =
            match self
                .pool
                .allocate(AtlasFormat::R8, Tenant::CutoutTables, U16Vec2::splat(edge))
            {
                Ok(handle) => handle,
                Err(AtlasError::Full) => return None,
                Err(oversized @ AtlasError::Oversized { .. }) => {
                    unreachable!("a table fits a mask page by the plan's bound: {oversized}")
                }
            };
        self.pool.pin(handle);
        placements.keys.insert(
            key,
            PlacedTable {
                handle,
                side,
                baked: None,
            },
        );
        placements.by_handle.insert(handle, key);
        Some(self.code(key))
    }

    /// The [`CutoutPlan::code`] of `key`, which a plan holds a use of.
    pub(crate) fn code(&self, key: CutoutKey) -> u32 {
        let placed = self.placements.keys[&key];
        let placement = self
            .pool
            .placement(placed.handle)
            .expect("a held table is pinned");
        CutoutPlan::code(placement, placed.side)
    }

    /// Give back one use of `key`, which [`Self::acquire`] placed, and stamp it as drawn now, so
    /// the clock takes the tables let go longest ago first.
    pub(crate) fn release(&mut self, key: CutoutKey) {
        let handle = self.placements.keys[&key].handle;
        self.pool.unpin(handle);
        let stamped = self.pool.stamp(handle);
        debug_assert!(stamped.is_some(), "a table pinned until now is placed");
    }
}

impl CutoutPlacements {
    /// Write into `bakes` what of the frame's `tables` the mask pages lack, and note it baked: a
    /// table baked at its place whole bakes nothing, one baked there in part bakes the least span
    /// that holds both, and one placed since its last bake bakes its span.
    pub(crate) fn bakes(&mut self, tables: &[BakeTable], bakes: &mut Vec<BakeTable>) {
        bakes.clear();
        for &table in tables {
            let placed = self
                .keys
                .get_mut(&table.key())
                .expect("a planned table is placed");
            let bake = match placed.baked {
                Some(baked) if baked.is_at(table) && baked.holds(table) => continue,
                Some(baked) if baked.is_at(table) => baked.joined(table),
                _ => table,
            };
            placed.baked = Some(bake);
            bakes.push(bake);
        }
    }

    /// Drop the tables the pool evicted since the last drain, at the end of each submit.
    pub(crate) fn drain_evicted(&mut self, pool: &mut AtlasPool) {
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
    use crate::gpu::atlas::page_list::PageListConfig;
    use crate::gpu::atlas::tenant::Tenant;
    use crate::gpu::pipeline::quad_pipeline::cutout_placements::{CutoutPlacements, CutoutPlacing};
    use crate::gpu::pipeline::quad_pipeline::cutout_plan::{BakeTable, CutoutKey, CutoutPlan};
    use glam::U16Vec2;

    fn key(r: f32) -> CutoutKey {
        CutoutKey::new(r, 2.0)
    }

    /// A pool whose mask list holds one 64² page and evicts past nothing.
    fn pool() -> AtlasPool {
        let config = PageListConfig {
            first_edge: 64,
            page_edge: 64,
            soft_bytes: 0,
        };
        AtlasPool::with_configs([config; 3], 16384)
    }

    /// The code of a table of `side` at `(x, y)` of page 0.
    fn at(x: u16, y: u16, side: u32) -> u32 {
        on(0, x, y, side)
    }

    /// The code of a table of `side` at `(x, y)` of page `page`.
    fn on(page: u8, x: u16, y: u16, side: u32) -> u32 {
        CutoutPlan::code(
            Placement {
                page,
                origin: U16Vec2::new(x, y),
                size: U16Vec2::splat(side as u16),
            },
            side,
        )
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
            .allocate(AtlasFormat::R8, Tenant::Glyphs, U16Vec2::new(64, 32))
            .unwrap();
        assert_eq!(pool.placement(glyph).unwrap().origin, U16Vec2::new(0, 32));
        assert!(
            pool.allocate(AtlasFormat::R8, Tenant::Glyphs, U16Vec2::splat(32))
                .is_ok(),
            "b's texels"
        );
        placements.drain_evicted(&mut pool);
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
            .allocate(AtlasFormat::R8, Tenant::Glyphs, U16Vec2::splat(64))
            .unwrap();
        pool.release(glyph);
        let mut placing = CutoutPlacing {
            placements: &mut placements,
            pool: &mut pool,
        };
        assert_eq!(placing.acquire(key(1.0), 64), Some(at(0, 0, 64)));
        assert_eq!(placing.users(key(1.0)), 1);
        placements.drain_evicted(&mut pool);
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
    /// in part, already baked, and bakes nothing; once `b` is placed again its old bake does not
    /// count, and the table bakes its span.
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
        assert_eq!(bakes, [moved], "another place bakes afresh");
    }
}
