//! Cross-frame `WidgetId → Any` state. Widgets that need memory beyond
//! the frame they're recorded in (scroll offset, focus, animation
//! progress, text editor) read/write a typed row keyed by their
//! `WidgetId`. Eviction follows the same `SeenIds.removed` diff that
//! the text and layout caches consume — when a widget stops being
//! recorded, its row is dropped at `Ui::post_record`.
//!
//! Steady-state allocation: one `Box` per widget on first insert.
//! Subsequent frames are pure hashmap probes — no allocs after warmup,
//! pinned by the `tests/alloc` fixture.
//!
//! Type discipline: a `WidgetId` carries a single concrete `T` for its
//! lifetime. Reading or writing with a mismatching `T` is a logic bug
//! (id collision or accidental reuse) and panics rather than silently
//! returning a fresh default.

use crate::forest::widget_id::WidgetId;
use rustc_hash::{FxHashMap, FxHashSet};
use std::any::Any;

#[derive(Default)]
pub(crate) struct StateMap {
    rows: FxHashMap<WidgetId, Box<dyn Any>>,
}

impl StateMap {
    pub(crate) fn get_or_insert_with<T, F>(&mut self, id: WidgetId, init: F) -> &mut T
    where
        T: 'static,
        F: FnOnce() -> T,
    {
        let row = self.rows.entry(id).or_insert_with(|| Box::new(init()));
        row.downcast_mut::<T>().unwrap_or_else(|| {
            panic!(
                "StateMap row for {id:?} was inserted with a different type than \
                 requested ({}); WidgetId collision or reuse",
                std::any::type_name::<T>(),
            )
        })
    }

    pub(crate) fn sweep_removed(&mut self, removed: &FxHashSet<WidgetId>) {
        for id in removed {
            self.rows.remove(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wid(n: u64) -> WidgetId {
        WidgetId::from_hash(n)
    }

    #[test]
    fn value_persists_across_frames() {
        let mut map = StateMap::default();
        *map.get_or_insert_with(wid(1), || 0u32) = 42;
        assert_eq!(*map.get_or_insert_with(wid(1), || 0u32), 42);
    }

    #[test]
    fn init_only_runs_on_first_insert() {
        let mut map = StateMap::default();
        let mut init_calls = 0u32;
        for _ in 0..3 {
            let _ = map.get_or_insert_with(wid(1), || {
                init_calls += 1;
                7u32
            });
        }
        assert_eq!(init_calls, 1);
    }

    #[test]
    fn sweep_removed_drops_rows() {
        let mut map = StateMap::default();
        *map.get_or_insert_with(wid(1), || 0u32) = 99;
        map.sweep_removed(&FxHashSet::from_iter([wid(1)]));
        // Re-inserting yields the init value, not the swept-away 99.
        assert_eq!(*map.get_or_insert_with(wid(1), || 0u32), 0);
    }

    #[test]
    #[should_panic(expected = "different type")]
    fn type_mismatch_on_get_or_insert_panics() {
        let mut map = StateMap::default();
        let _ = map.get_or_insert_with(wid(1), || 0u32);
        let _ = map.get_or_insert_with(wid(1), || 0u64);
    }
}
