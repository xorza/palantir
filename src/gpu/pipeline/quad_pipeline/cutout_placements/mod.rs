//! Where each `(r, σ)` cutout table lives in the pool's mask pages, and which of its texels are
//! baked.

use crate::common::gen_slab::{GenSlab, SlabKey};
use crate::gpu::atlas::AtlasPool;
use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::atlas::atlas_handle::{AtlasHandle, Evicted};
use crate::gpu::atlas::error::AtlasError;
use crate::gpu::atlas::tenant::{ItemOwner, Tenant};
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
    keys: FxHashMap<CutoutKey, SlabKey>,
    /// Each placed table; a table's pool item carries its entry's slot as its tag, so an eviction
    /// the pool queues finds its entry.
    tables: GenSlab<PlacedTable>,
}

#[derive(Clone, Copy, Debug)]
struct PlacedTable {
    key: CutoutKey,
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
        if let Some(&slab_key) = placements.keys.get(&key) {
            let handle = placements
                .tables
                .get(slab_key)
                .expect("a key names its table")
                .handle;
            if let Some(placement) = self.pool.pin(handle) {
                return Some(CutoutPlan::code(placement));
            }
            // Evicted, its eviction waiting in the queue, which the drain finds names a vacant slot
            // or another table's.
            placements.tables.remove(slab_key);
            placements.keys.remove(&key);
        }
        let pool = &mut *self.pool;
        let mut placed = None;
        let filed = placements.tables.try_insert_with_key(|slab_key| {
            let owner = ItemOwner {
                tenant: Tenant::CutoutTables,
                tag: slab_key.idx,
            };
            let allocation = pool.allocate(AtlasFormat::R8, owner, UVec2::splat(side))?;
            placed = Some(allocation.placement);
            Ok(PlacedTable {
                key,
                handle: allocation.handle,
                baked: None,
            })
        });
        let slab_key = match filed {
            Ok(slab_key) => slab_key,
            Err(AtlasError::Full) => return None,
            Err(oversized @ AtlasError::Oversized { .. }) => {
                unreachable!("a table fits a mask page by the plan's bound: {oversized}")
            }
        };
        placements.keys.insert(key, slab_key);
        let handle = placements.tables.get(slab_key).expect("filed above").handle;
        let pinned = pool.pin(handle);
        debug_assert_eq!(pinned, placed, "a new table is placed");
        Some(CutoutPlan::code(placed.expect("a filed table was placed")))
    }

    /// The [`CutoutPlan::code`] of `key`, which a plan holds a use of.
    pub(crate) fn code(&self, key: CutoutKey) -> u32 {
        let placement = self
            .pool
            .placement(self.placements.table(key).handle)
            .expect("a held table is pinned");
        CutoutPlan::code(placement)
    }

    /// Give back one use of `key`, which [`Self::acquire`] placed; the unpin stamps it as drawn
    /// now, so the clock takes the tables let go longest ago first.
    pub(crate) fn release(&mut self, key: CutoutKey) {
        self.pool.unpin(self.placements.table(key).handle);
    }
}

impl CutoutPlacements {
    /// The table `key` holds, which a plan placed.
    fn table(&self, key: CutoutKey) -> &PlacedTable {
        self.tables
            .get(self.keys[&key])
            .expect("a key names its table")
    }

    /// Write into `bakes` what of the frame's `tables` the mask pages lack, and note it baked: a
    /// table baked whole bakes nothing, one baked in part bakes the least span that holds both,
    /// and one placed since its last bake, a new entry, bakes its span. A drain's move keeps the
    /// entry and its span, as the commit moved its texels.
    pub(crate) fn bakes(&mut self, tables: &[BakeTable], bakes: &mut Vec<BakeTable>) {
        bakes.clear();
        for &table in tables {
            let placed = self
                .tables
                .get_mut(self.keys[&table.key()])
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
    /// a purge. An eviction's tag names its table's slot, which still holds the evicted table
    /// unless a lookup that found it gone dropped it, and the slot took another since.
    pub(crate) fn drop_evicted(&mut self, pool: &mut AtlasPool) {
        for Evicted { handle, tag } in pool.drain_evicted(Tenant::CutoutTables) {
            let Some(slab_key) = self.tables.key_at(tag as usize) else {
                continue;
            };
            let table = self.tables.get(slab_key).expect("a slot's key names it");
            if table.handle == handle {
                self.keys.remove(&table.key);
                self.tables.remove(slab_key);
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
            self.placements.keys.get(&key).map_or(0, |&slab_key| {
                let placed = self
                    .placements
                    .tables
                    .get(slab_key)
                    .expect("a key names its table");
                self.pool
                    .list(AtlasFormat::R8)
                    .pins(placed.handle)
                    .unwrap_or(0)
            })
        }

        /// The display frame each placed table was last drawn in, in order.
        pub(crate) fn last_uses(&self) -> Vec<u64> {
            let list = self.pool.list(AtlasFormat::R8);
            let mut last_uses: Vec<u64> = self
                .placements
                .tables
                .values()
                .filter_map(|placed| list.last_use(placed.handle))
                .collect();
            last_uses.sort_unstable();
            last_uses
        }

        /// The uses every plan holds, of every key.
        pub(crate) fn uses(&self) -> u32 {
            let list = self.pool.list(AtlasFormat::R8);
            self.placements
                .tables
                .values()
                .filter_map(|placed| list.pins(placed.handle))
                .sum()
        }
    }

    impl CutoutPlacements {
        /// The tables baked in part or whole.
        pub(crate) fn baked_len(&self) -> usize {
            self.tables
                .values()
                .filter(|placed| placed.baked.is_some())
                .count()
        }
    }
}

#[cfg(test)]
mod tests;
