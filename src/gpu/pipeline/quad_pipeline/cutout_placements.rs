//! Where each `(r, σ)` cutout table lives in the atlas every window's shadows read.

use crate::gpu::pipeline::quad_pipeline::cutout_plan::{CutoutKey, CutoutPlan};
use crate::gpu::resource::shelf_packer::ShelfPacker;
use glam::{U16Vec2, UVec2};
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::rc::Rc;

/// The cells each cutout key holds in the backend's one atlas, shared by every window's
/// [`CutoutPlan`] on the main thread, as `SharedGradientAtlas` is.
///
/// A placement stays while any plan tables its key, so a table keeps its cells across frames and
/// windows and the atlas bakes it once. A key no plan tables keeps its cells too, until a new key
/// needs the room: the least recently released goes first. Refused: packing each window's tables
/// afresh every frame, which moved a table whenever a key before it came or went and made two
/// windows with different shadows bake over each other every frame.
#[derive(Clone, Debug, Default)]
pub(crate) struct SharedCutoutPlacements(Rc<RefCell<CutoutPlacements>>);

#[derive(Debug)]
struct CutoutPlacements {
    /// Places rectangles of whole cells, in texels.
    packer: ShelfPacker,
    keys: FxHashMap<CutoutKey, Placement>,
    /// How many releases ran, the stamp a released placement keeps.
    releases: u64,
}

#[derive(Clone, Copy, Debug)]
struct Placement {
    /// [`CutoutPlan::code`] of the cells.
    code: u32,
    /// The plans that table the key.
    users: u32,
    /// [`CutoutPlacements::releases`] when the last user let go.
    released: u64,
}

impl Default for CutoutPlacements {
    fn default() -> Self {
        Self {
            packer: ShelfPacker::new(CutoutPlan::ATLAS_SIZE as u16),
            keys: FxHashMap::default(),
            releases: 0,
        }
    }
}

impl SharedCutoutPlacements {
    /// Take a use of `key`'s table of `side` texels: its [`CutoutPlan::code`], placed now where it
    /// has none, after the keys no plan tables, least recently released first, give up their
    /// cells as needed. `None` when even then no room holds it.
    pub(crate) fn acquire(&self, key: CutoutKey, side: u32) -> Option<u32> {
        let mut placements = self.0.borrow_mut();
        if let Some(placed) = placements.keys.get_mut(&key) {
            placed.users += 1;
            return Some(placed.code);
        }
        let cells = side.div_ceil(CutoutPlan::CELL);
        let edge = u16::try_from(cells * CutoutPlan::CELL).ok()?;
        let origin = loop {
            if let Some(origin) = placements.packer.allocate(U16Vec2::splat(edge)) {
                break origin;
            }
            if !placements.evict_released() {
                return None;
            }
        };
        // Every rectangle is whole cells, so every shelf, span and origin falls on a cell.
        debug_assert!(
            origin.as_uvec2() % CutoutPlan::CELL == UVec2::ZERO,
            "a placement at {origin:?} off the cell grid"
        );
        let code = CutoutPlan::code(origin.as_uvec2() / CutoutPlan::CELL, side);
        placements.keys.insert(
            key,
            Placement {
                code,
                users: 1,
                released: 0,
            },
        );
        Some(code)
    }

    /// The [`CutoutPlan::code`] of `key`, which a plan holds a use of.
    pub(crate) fn code(&self, key: CutoutKey) -> u32 {
        self.0
            .borrow()
            .keys
            .get(&key)
            .expect("a held key is placed")
            .code
    }

    /// Give back one use of `key`, which [`Self::acquire`] placed.
    pub(crate) fn release(&self, key: CutoutKey) {
        let mut placements = self.0.borrow_mut();
        placements.releases += 1;
        let releases = placements.releases;
        let placed = placements
            .keys
            .get_mut(&key)
            .expect("a released key was acquired");
        debug_assert!(placed.users > 0, "a key released more than acquired");
        placed.users -= 1;
        if placed.users == 0 {
            placed.released = releases;
        }
    }
}

impl CutoutPlacements {
    /// Free the cells of the least recently released key no plan tables; `false` when every
    /// placed key is in use.
    fn evict_released(&mut self) -> bool {
        let Some((&key, placed)) = self
            .keys
            .iter()
            .filter(|(_, placed)| placed.users == 0)
            .min_by_key(|(_, placed)| placed.released)
        else {
            return false;
        };
        let cells = CutoutPlan::cells(placed.code);
        self.packer.deallocate(
            (cells.min * CutoutPlan::CELL).as_u16vec2(),
            (cells.size.x * CutoutPlan::CELL) as u16,
        );
        self.keys.remove(&key);
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::pipeline::quad_pipeline::cutout_placements::SharedCutoutPlacements;
    use crate::gpu::pipeline::quad_pipeline::cutout_plan::{CutoutKey, CutoutPlan};
    use glam::UVec2;

    fn key(r: f32) -> CutoutKey {
        CutoutKey::new(r, 2.0)
    }

    /// A key keeps its cells while a plan uses it, and after, until a new key needs the room. A
    /// 256-texel table takes 16 cells a side, so the 64-cell atlas holds 16. Two uses of `a` share
    /// one placement at the origin; a second key lands beside it. Sixteen fill the atlas; with
    /// every key in use a seventeenth finds no room. Released keys go least recently released
    /// first: `b`, released before `c`, gives up its cells (16, 0) to the newcomer, and `c` keeps
    /// its own.
    #[test]
    fn placements_stay_until_a_new_key_needs_the_room() {
        let placements = SharedCutoutPlacements::default();
        let at = |x: u32, y: u32| CutoutPlan::code(UVec2::new(x, y), 256);
        assert_eq!(placements.acquire(key(1.0), 256), Some(at(0, 0)));
        assert_eq!(placements.acquire(key(1.0), 256), Some(at(0, 0)), "shared");
        assert_eq!(placements.acquire(key(2.0), 256), Some(at(16, 0)));
        assert_eq!(placements.acquire(key(3.0), 256), Some(at(32, 0)));
        for k in 4..=16 {
            assert!(placements.acquire(key(k as f32), 256).is_some(), "key {k}");
        }
        assert_eq!(placements.acquire(key(17.0), 256), None, "every key in use");

        placements.release(key(2.0));
        placements.release(key(3.0));
        assert_eq!(placements.acquire(key(3.0), 256), Some(at(32, 0)), "kept");
        placements.release(key(3.0));
        assert_eq!(
            placements.acquire(key(17.0), 256),
            Some(at(16, 0)),
            "b's cells"
        );
        assert_eq!(
            placements.acquire(key(3.0), 256),
            Some(at(32, 0)),
            "c kept its own"
        );
    }
}
